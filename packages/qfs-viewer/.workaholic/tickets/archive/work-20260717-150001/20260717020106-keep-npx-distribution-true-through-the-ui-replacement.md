---
created_at: 2026-07-17T02:01:06+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure, Config]
effort: 2h
commit_hash:
category: Changed
depends_on: [20260717020104-strip-ui-on-the-plggmatic-engine.md]
mission: qfs-viewer-mvp
---

# Distribution: `npx qfs-viewer` stays true through the UI replacement

## Overview

Mission acceptance item 6 (demo leg 1). The bin exists and is smoke-tested
today; this ticket keeps that promise through the strip-UI replacement and
the qfs dependency: `npx qfs-viewer` at any repository root starts the
viewer, with the qfs binary reached per the plan (bundled/auto-fetched à la
esbuild's npm binary-distribution pattern, or found on PATH — decide and
record which the MVP ships, and what the error says when no qfs is
reachable).

## Policies

- `workaholic:operation` / `policies/ci-cd.md` — the npx smoke stays the
  gate's teeth: pack, install into a scratch tree, run under every available
  runtime; a silent skip is the named regression.
- `workaholic:design` / `policies/user-sovereignty.md` — the issuance-form
  choice (local/on-demand/remote) stays the user's; distribution must not
  hard-wire one.
- `workaholic:implementation` / `policies/objective-documentation.md` — the
  qfs-binary acquisition decision is an ADR (it has real alternatives and
  real costs).

## Quality Gate

- Acceptance: `npx qfs-viewer serve` (packed tarball, scratch tree) starts
  and serves the strip UI; with no qfs reachable it starts and says exactly
  what is missing and how to get it, rather than crashing.
- Verification: `scripts/smoke-npx.sh` extended to assert the serve path,
  not only `--version`/`--help`.
- Gate: `./scripts/check-all.sh` exits 0.
