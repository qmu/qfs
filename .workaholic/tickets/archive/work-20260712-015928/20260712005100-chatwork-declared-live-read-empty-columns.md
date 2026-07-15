---
created_at: 2026-07-12T00:51:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Infrastructure]
effort: 2h
commit_hash:
category: Changed
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Chatwork declared driver live read returns rows with zero columns

## Overview

Found live during the owner-attended rounds (2026-07-11). Reading the DECLARED Chatwork driver's
rooms view against the real API returns the right ROW COUNT but every row is EMPTY — the column
values are lost between the HTTP JSON decode and the rendered relation:

```sh
qfs run '/chatwork/rooms |> limit 5'            # → (0 columns, 5 row(s))
qfs run '/chatwork/rooms |> select room_id, name'  # → rows of {}
```

The declared view is `/http/chatwork/rooms |> decode json` typed by `/type/chatwork/room`
(`room_id int pk, name text`). The hermetic twin (`crates/parser/tests/slack_twin.rs` family /
declared-driver tests) covers install + shape, evidently NOT the live decode→typed-view column
mapping — the gap is exactly where the mock stops.

## Repro

Live: `qfs run '/chatwork/rooms |> limit 3'` with the connected `vault:chatwork/work` token —
rows present, schema `[]`, every row `{}`.

## Key Files

- `packages/qfs/crates/exec/src/declared.rs` - the declared-driver read path (view body → HTTP → decode → typed relation)
- `packages/qfs/crates/qfs/src/declared_driver.rs` - live wiring (secrets, base_url, view registry)
- `packages/qfs/crates/parser/tests/fixtures/` - the chatwork.qfs declaration the live config installed

## Implementation Steps

1. Trace the live path: does the `/http/...` GET body reach `decode json`? Does the `of_type`
   schema apply its columns to the decoded rows, or does an empty described schema project
   everything away?
2. Suspect: the typed-view projection maps columns BY NAME against a schema the decode step never
   attached (works in hermetic tests because the mock supplies a schema-carrying batch).
3. Hermetic lock at the decode→typed-view seam with a JSON body fixture shaped like the real
   Chatwork `/rooms` response (array of objects, surplus fields included).
4. Owner-attended re-check of `/chatwork/rooms` after the fix.

## Quality Gate

- Hermetic decode→typed-view test green; live `/chatwork/rooms` shows `room_id`/`name` values.
