---
created_at: 2026-07-17T02:01:01+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort: 2h
commit_hash:
category: Added
depends_on:
mission: qfs-viewer-mvp
---

# The qfs connection seam: on-demand spawn as the default, form swappable by configuration

## Overview

Mission acceptance item 5 (demo leg 1). The viewer's every qfs query must go
through ONE seam whose form is chosen by configuration, per the plan's three
issuance forms (`qmu/strategy` `docs/plan.md`): ① a locally running qfs
server, ② an on-demand command invocation per query, ③ a remote qfs. The MVP
implements ② fully and only SHAPES the seam for ① and ③ — a skeleton form is
selectable in config and answers with a typed "not implemented" error rather
than not existing, so the swap point is proven by construction before either
form is real.

## Policies

- `workaholic:implementation` / `policies/type-driven-design.md` — the
  connection is a closed union folded exhaustively; an unimplemented form is
  a typed error value, never a throw or a silent fallback.
- `workaholic:implementation` / `policies/anti-corruption-structure.md` —
  the union and its parser are domain; the process spawn stays in
  `vendors/`, behind the existing `ResourceRunner` seam.
- `workaholic:design` / `policies/sacrificial-architecture.md` — the seam is
  the sacrificial unit: forms ① and ③ are skeletons whose arrival replaces
  one fold arm, not the callers.

## What exists

- `src/vendors/qfsRunner.ts` already spawns the `qfs` binary per query
  (`execFileSync`, statement as one argv element, structured-error-on-stdout
  handling) — form ②, hard-wired.
- `src/domain/model/Scan.ts` holds the `ResourceRunner` seam (`run` only).
- `qfs-viewer.config.json` (`src/domain/model/Config.ts`) has no connection
  vocabulary.

## Implementation

1. `src/domain/model/Connection.ts`: a closed `QfsConnection` union —
   `Spawn{bin}` (default, `bin: "qfs"`) / `LocalServer{url}` / `Remote{url}`
   — with `asQfsConnection` parsing the config's `qfs` key
   (`{"form": "spawn"|"local-server"|"remote", "bin"?, "url"?}`); reject,
   don't repair, matching `asConfig`'s stance.
2. `Config` gains the `qfs: QfsConnection` field (absent = `Spawn`).
3. `ResourceRunner` gains `describe(path)` beside `run(statement)` — the
   describe→preview loop is qfs's published interface and generic browsing
   (the next ticket) needs the first half.
4. `qfsRunner(connection)` folds the union: `Spawn` spawns
   `<bin> --json run|describe <arg>` per query; `LocalServer`/`Remote`
   return a typed error naming the unimplemented issuance form.
5. `serve.ts` builds the runner from the loaded config's connection.

## Quality Gate

- Acceptance: a config with `{"qfs": {"form": "spawn", "bin": "/opt/qfs"}}`
  spawns that binary; no config spawns `qfs`; `local-server`/`remote`
  configs are accepted and answer queries with the typed skeleton error.
- Verification: unit specs without a real qfs (fixture executable for the
  spawn form; the skeleton forms are pure), plus a live spot-check against
  the installed binary.
- Gate: `./scripts/check-all.sh` exits 0.
