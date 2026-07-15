---
created_at: 2026-07-04T16:43:15+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort: 4h
commit_hash: 23aab40
category: Changed
depends_on: []
---

# Bug: `/local`→Drive (and `/local`→`/local`) blob copy fails at COMMIT — single-file `Read` lowered to `read_dir` (ENOTDIR); and gdrive UPSERT into My Drive root can't resolve `parent_id`

## Summary

Uploading a local file to Google Drive is impossible in the installed **qfs 0.0.17 (commit
`94dea14`, `aarch64-unknown-linux-musl`)**. Two independent commit-time defects block it. In both
cases `describe` and `preview` succeed and produce a sensible plan — only `--commit` fails. **Reads
work; the write/copy apply legs are the gap.** Found while copying a generated PDF report into
`/drive/my/`.

## Environment

- `qfs 0.0.17`, commit `94dea14`, target `aarch64-unknown-linux-musl`, statically linked.
- `/drive` bound to the `gdrive` driver (account `a@qmu.jp`, `drive` consent); `/local` built-in.
- Vault unlocks fine (`QFS_PASSPHRASE` accepted — auth is **not** the problem).

## Bug 1 — single-file `/local` `Read` at commit is lowered to a directory `read_dir` → `NotADirectory`

Reproduces with **no network and no credentials** (local→local):

```sh
printf 'hello qfs\n' > /home/ec2-user/hello.txt
qfs run "/local/home/ec2-user/hello.txt
|> upsert into /local/home/ec2-user/hello-copy.txt" --commit --json
```
```json
{"error":{"code":"commit_failed","kind":"commit_failed",
 "message":"Retryable { reason: \"io error at \\\"/local/home/ec2-user/hello.txt\\\": NotADirectory\" }; skipped (dependency NodeId(0) failed)"}}
```

The cross-service form fails identically (same `NotADirectory` on the `/local` source):

```sh
qfs run "upsert into /drive/my/report.pdf /local/home/ec2-user/report.pdf" --commit --json
```

Yet a **bare single-file read returns the bytes correctly** (so reading is fine; only the
commit/materialisation path is broken):

```sh
qfs run "/local/home/ec2-user/hello.txt" --json
# {"rows":[{"name":"hello.txt",...,"is_dir":false,...,"content":[104,101,108,108,111,32,113,102,115,10]}]}
```

### Root cause

`LocalEffect::from_node` maps **every** `Read`/`List` to a directory scan:

- `crates/driver-local/src/effect.rs:82` — `EffectKind::Read | EffectKind::List => Ok(LocalEffect::Scan { path })`
- The applier runs `Scan` via `fs_core::scan_dir` (`crates/driver-local/src/applier.rs:44-51`).
- `scan_dir` calls `fs::read_dir(&abs)` (`crates/driver-local/src/fs_core.rs:221`). On a **regular
  file**, `read_dir` returns `ErrorKind::NotADirectory` (ENOTDIR), wrapped by
  `LocalError::from_io(vfs_dir, …)` → the `NotADirectory` message above.

The query/read leg already has the correct primitive — `fs_core::read_blob`
(`crates/driver-local/src/fs_core.rs:376`) — which is why the bare read returns `content`. The
commit path just never calls it for a single-file source; it always scans a directory.

## Bug 2 — gdrive UPSERT into My Drive root can't resolve `parent_id`

Isolates the Drive **write** path from Bug 1 (literal bytes, so no `/local` read involved):

```sh
qfs run "upsert into /drive/my/qfs-test.txt values ('hello from qfs 0.0.17')" --commit --json
```
```json
{"error":{"code":"commit_failed","kind":"commit_failed",
 "message":"Terminal { reason: \"malformed INSERT effect at \\\"/drive/my/qfs-test.txt\\\": upload needs the resolved `parent_id`\" }"}}
```

### Root cause

The commit path decodes the upload effect through the **fail-closed `NoResolve`** resolver instead
of a live `WriteResolver`:

- `crates/driver-gdrive/src/effect.rs:57-75` — `NoResolve::folder_id` returns
  `DriveError::MalformedEffect { reason: "upload needs the resolved `parent_id`" }` (message at
  `:68`, column const `PARENT_ID_COL` at `:78`).

So the live resolver that would map `/drive/my` → the My Drive root parent id (`root`) — and nested
folders → their ids — is not wired into the gdrive apply leg (or root-folder resolution is
unimplemented). See `crates/driver-gdrive/src/{applier.rs,read.rs}` for where the live
`WriteResolver` should be threaded into `from_node` at commit.

## Expected behaviour

- `upsert into /local/<dst> /local/<src>` and `upsert into /drive/my/<name> /local/<path>` **commit**,
  copying/uploading the source bytes.
- A single-file `/local` `Read` yields a `content` row (bytes), not a directory `read_dir`.
- UPSERT into `/drive/my/<name>` resolves the My Drive root `parent_id` at apply time.

## Key files

- `crates/driver-local/src/effect.rs:82` — the defect: `Read | List → Scan` (no single-file branch).
- `crates/driver-local/src/fs_core.rs:220-222` — `scan_dir` → `fs::read_dir` (ENOTDIR on a file).
- `crates/driver-local/src/fs_core.rs:376` — `read_blob`, the correct single-file read to reuse.
- `crates/driver-local/src/applier.rs:43-68` — `apply_effect` (`Scan`/`Copy`/`Write`/`Move`/`Remove`).
- `crates/driver-gdrive/src/effect.rs:54-78` — `NoResolve::folder_id` + `PARENT_ID_COL`.
- `crates/driver-gdrive/src/{applier.rs,read.rs}` — live `WriteResolver` wiring for commit.

## Settled design (2026-07-04, owner-approved — supersedes the "Suggested direction" fork)

**Do NOT implement either fork that was previously on the table** (A: same-driver SRC_COL `Copy`
lowering + separate cross-driver path; B: file-aware `Scan` + runtime row-threading). The
investigation established the real gap is general: the effect plan's source `Read` node is only a
*dependency marker* and the runtime has **no row channel between effects** (`EffectOutput` =
`{id, affected}`; the audit ledger is metadata-only by rule) — the `INSERT … FROM <pipeline>`
commit-side materialisation named as a pre-existing gap in archived ticket 20260701192440. The
ENOTDIR is just where it falls over first.

**Bug 1 — implement commit-boundary materialization (blueprint §7, decided 2026-07-04):**

- At `--commit`, above the interpreter, the exec/binary layer (the one layer holding both the
  statement's query side and the effect plan) re-executes the pipeline/`FROM` source through the
  **existing read engine** (already cross-driver; the bare read demonstrably returns the bytes)
  and embeds the produced rows into the write effect's `args.rows` — the same channel `VALUES`
  writes already use (`node_from_input` carries non-empty `args.rows`).
- The source `Read` node is consumed there: ledgered as applied with affected = materialized row
  count, never dispatched to a driver — so `LocalEffect::Scan` never sees a single file and no
  `ReadBlob` effect is needed.
- The interpreter/driver contract stays payload-free; no engine→runtime inversion.
- Add a **named payload cap** (constant) refusing over-size materializations with a structured
  error naming the in-driver remedy (`cp` verb, `CALL drive.copy`). Same-driver `Copy` pushdown
  and streaming are named parks — optimizations, not correctness.
- e2e: `/local`→`/local` file copy (creds-free, deterministic) and the cross-driver form both
  commit; preview==commit shape holds; the ledger shows both legs honestly.

**Bug 2 — ALREADY FIXED** on the v0.0.19 branch by the v0.0.18 `WriteResolver` work: verify the
repro (`upsert into /drive/my/qfs-test.txt values (…)` commits) and reflect it closed here —
do not re-implement.

**Fold-in:** `describe /drive/my` under-reports `verbs.upsert` (the Considerations note) — fix
the describe verb set alongside.

## Considerations

- These line up with the cookbook's own 🚧 markers ("Write & copy", "Move data between services",
  cross-service materialisation "still being wired"). Likely related to the in-flight
  `20260630203000-epic-replace-gmail-ftp-gdrive-ftp.md` — link/dedupe rather than duplicate.
- The effect **plan is correct** (preview shows `READ` + `UPSERT` with the right targets); the fix is
  in the **apply legs**, not the planner.
- Minor inconsistency worth a glance while here: `qfs describe /drive/my` reports
  `verbs.upsert=false` (native `SELECT LS`), yet `preview`/`commit` accept `UPSERT` into a
  `/drive/my/<file>` path — the `describe` verb set under-reports the write surface.
- Bug 1 repro is deterministic and creds-free (local→local); Bug 2 needs a connected Drive account.
