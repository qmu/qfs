---
created_at: 2026-07-07T18:14:04+09:00
author: a@qmu.jp
type: bugfix
layer: [Infrastructure]
effort:
commit_hash: 2120eb8
category: Changed
depends_on:
mission:
---

# Fix Drive cp from /local uploading zero-byte files

## Overview

Uploading a local PDF to Google Drive through the interactive qfs shell `cp`
path created or replaced the Drive file but left it at zero bytes. The same file
uploaded correctly through a direct `upsert into /drive/... values ('...')`
statement, so the bug appears to be in the local-file-to-Drive copy/materialize
path rather than the Drive UPSERT endpoint itself.

Observed reproduction on 2026-07-07:

```sh
printf 'cp /local/tmp/report.pdf /drive/my/report.pdf\nCOMMIT\n' | qfs
qfs run "/drive/my/report.pdf |> select name, mime_type, size, md5" --json
```

The metadata read returned `mime_type: application/pdf`, `size: 0`, and
`md5: d41d8cd98f00b204e9800998ecf8427e`. Replacing the same path with a direct
Drive `UPSERT` of the PDF bytes produced `size: 6891`, proving the Drive target
can store the bytes when they are present in the write effect.

## Policies

The standard engineering policies that govern this ticket:

- `workaholic:implementation` / `policies/directory-structure.md` — keep the fix
  in the existing host planning, local driver, Drive driver, or runtime apply
  layer that owns the defect.
- `workaholic:implementation` / `policies/coding-standards.md` — preserve typed,
  explicit byte handling across the copy boundary; do not hide missing content
  behind assertions or untyped row values.
- `workaholic:operation` / `policies/ci-cd.md` — add a local regression check so
  the upload path is reproducible without relying on a hosted CI signal.

## Key Files

- `packages/qfs/crates/host/src/derive.rs` - likely interactive shell `cp`
  parsing/planning path; verify how source paths become effect args.
- `packages/qfs/crates/driver-local/src/effect.rs` - defines how local copy
  source metadata is carried in effect rows.
- `packages/qfs/crates/driver-local/src/applier.rs` - helper constructors for
  local blob write and copy args; useful reference for intended byte semantics.
- `packages/qfs/crates/driver-gdrive/src/effect.rs` - decodes Drive UPSERT
  effects; confirm whether it receives byte content or only an empty row.
- `packages/qfs/crates/driver-gdrive/src/applier.rs` - applies Drive uploads;
  should reject or fail loudly when a blob upload is missing bytes.
- `packages/qfs/crates/driver-gdrive/src/tests.rs` - add or extend tests for
  Drive UPSERT decoding and local-to-Drive copy behavior.
- `docs/cookbook/gdrive.md` - currently recommends `printf 'cp /local/... /drive/...';
  keep documentation aligned with the fixed behavior.

## Implementation Steps

1. Reproduce the defect with a small non-empty fixture file and a Drive-like fake
   or integration seam: plan `cp /local/tmp/source.pdf /drive/my/source.pdf`,
   commit, then assert the destination receives the exact source bytes.
2. Trace the generated effect plan for shell `cp` across different blob drivers.
   The destination Drive UPSERT must carry concrete bytes, a stream source that
   is materialized at commit, or an explicit cross-driver copy operation that
   reads the source bytes before upload.
3. Fix the responsible boundary so local-file copy into Drive cannot silently
   become an empty upload. If byte materialization fails, return a terminal
   commit error rather than creating a zero-byte object.
4. Add regression coverage for:
   - local file with non-zero bytes copied to Drive receives the same byte count;
   - empty local file copied to Drive remains a valid intentional zero-byte case;
   - missing or unreadable local source fails before replacing the Drive target;
   - direct Drive UPSERT behavior remains unchanged.
5. Update `docs/cookbook/gdrive.md` only if the user-facing command form changes.

## Considerations

- The failure mode is dangerous because qfs reported a committed Drive UPSERT
  while the uploaded artifact was empty. Treat silent truncation as a data
  integrity bug, not a cosmetic affected-count issue.
- Keep preview semantics pure: preview may show the planned source read and
  destination UPSERT, but real byte materialization belongs at commit.
- Preserve qfs's safety model. This is a reversible UPSERT, but it still must
  either upload the correct bytes or fail closed.

## Quality Gate

### Acceptance Criteria

- A non-empty local file copied to `/drive/...` through interactive shell `cp`
  produces a Drive blob whose size and content match the source.
- A failed source read or missing byte materialization does not create or replace
  the Drive object with an empty file.
- Direct `upsert into /drive/... values (...)` still uploads bytes correctly.
- The Google Drive cookbook command for local uploads is true after the fix.

### Verification Method

- Add a regression test at the lowest layer that can simulate the cross-driver
  local-to-Drive commit without live Google credentials.
- Add or update Drive effect/applier tests to prove empty-byte uploads are only
  accepted when the source file is actually empty.
- Run the qfs package test/check command used by this repository.

### Gate

- The new regression test fails before the fix and passes after it.
- Existing qfs tests pass.
- A manual live smoke test with a small PDF or text file confirms Drive metadata
  reports a non-zero size after `cp /local/... /drive/...` and `COMMIT`.
