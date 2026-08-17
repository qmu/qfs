---
created_at: 2026-08-17T11:15:30+00:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission: the-current-situation-of-qfs-is-documented-as-it-actually-stands
merge_policy:
verification_handoff:
---

# The qfs-viewer gate cannot pass where bun is installed

## Overview

Found while running the gate commands for ticket
`20260817102723-document-the-repository-as-it-stands-both-packages-the-gates-the-anti-drift-generators-and-the-release-path.md`.

`cd packages/qfs-viewer && ./scripts/check-all.sh` **exits 1** on any machine that has bun
installed. The two structural gates, the dist build and the node leg of the npx smoke all pass;
the bun leg fails inside a **published dependency**, before any of this repository's code runs:

```
=== Smoke: npx qfs-viewer (packed, installed, executed) ===
    serve: 1 document(s), engine strip, /resolve column, qfs advice — still serving
  PASS: node runs the packed bin from under node_modules (version 0.0.1) and serves the strip on :39533
  FAIL: bun could not run the packed bin:
    1121 | "src/Render/usecase/slugify.ts": function (module, exports, require) {
           ^
    SyntaxError: Invalid regular expression: range out of order in character class
          at <parse> (/tmp/qfs-viewer-smoke-wBzn8l/node_modules/plgg-md/dist/index.es.js:1124:1)
    Bun v1.3.11 (Linux x64)
```

The failing file is `plgg-md`'s **published dist**, not source in this tree. `plgg-md@0.0.3` is
the newest version on the registry (`npm view plgg-md versions` → `["0.0.2","0.0.3"]`), so the
declared `^0.0.3` already resolves to the newest and no bump can fix it. `scripts/smoke-npx.sh`
deliberately runs **every installed runtime** (node, bun, deno) and skips only absent ones out
loud, because the mission requires all three and a silent skip once let bun stay broken for a
whole session.

Two consequences worth separating:

- **The published artifact really is broken under bun 1.3.11** — the smoke is right to fail.
  `npx qfs-viewer` on a bun user's machine would hit exactly this.
- **The gate is environment-dependent in a way nothing states.** CI's `viewer-check-all` job sets
  up Node only, so bun and deno are absent there and the smoke skips them — CI is green while a
  developer container with bun installed is red on an unchanged tree. Neither `CLAUDE.md` nor
  `packages/qfs-viewer/README.md` says the gate's meaning depends on which runtimes the machine
  happens to carry.

## Policies

- `workaholic:implementation` / `policies/test.md` — what a gate must actually prove
- `workaholic:operation` / `policies/ci-cd.md` — the local gate and the CI backstop must mean the
  same thing
- `workaholic:design` / `policies/vendor-neutrality.md` — the runtime matrix is a product promise
- `workaholic:implementation` / `policies/objective-documentation.md` — a gate's preconditions are
  part of its documentation

## Key Files

- `packages/qfs-viewer/scripts/smoke-npx.sh` — the runtime loop (`for RUNTIME in node bun deno`)
  and the skip-out-loud rule; its own comments record why a silent skip was rejected.
- `packages/qfs-viewer/packages/qfs-viewer/package.json` — declares `plgg-md: ^0.0.3`.
- `.github/workflows/ci.yml` — the `viewer-check-all` job; installs Node only, so the matrix it
  exercises is narrower than a developer's.
- `packages/qfs-viewer/README.md`, `packages/qfs-viewer/CLAUDE.md` — both claim node, bun and
  deno without stating that the gate only covers the runtimes present.
- `docs/guide/repository.md` — describes the smoke; update it with whatever is decided.

## Implementation Steps

1. Reproduce and localize: run the smoke with bun alone, and confirm the failing construct is in
   `plgg-md/dist/index.es.js` (the bundled `slugify.ts` character class) and not in this
   repository's dist. Extract the offending regex literal so the upstream report is actionable.
2. Establish whether bun or the bundle is at fault: test the same character class in bun and in
   node directly. A class that node accepts and bun rejects is a bun `v`-flag/unicode-set
   difference; a genuinely inverted range is a bundler or source defect.
3. File it upstream against `plgg-md` (private `qmu/plgg`), with the extracted literal and both
   runtimes' behaviour. This is the fix that makes the shipped product work under bun.
4. Decide the local gate's contract while that is open, and record the decision in the script's
   own comments: keep failing (the artifact is broken, so red is truthful), or pin the failing
   runtime with a named, dated exemption naming the upstream issue. Do not silently drop bun.
5. Make CI's coverage match the claim, or the claim match CI: either install bun and deno in
   `viewer-check-all` so the promised matrix is actually exercised, or state in the README and
   `CLAUDE.md` that CI covers Node only.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- `cd packages/qfs-viewer && ./scripts/check-all.sh` exits 0 on a machine with bun installed, or
  exits 1 with a message naming the upstream issue and the dated exemption that governs it.
- The upstream report exists and is referenced from the script or the ADRs.
- CI's runtime coverage and the documented runtime claim agree — a reader can tell which runtimes
  a green CI run actually proved.
- No runtime is dropped from the smoke without a comment saying which issue authorized it and
  when it should be revisited.

**Verification method** — the commands/tests/probes that prove them:

- `cd packages/qfs-viewer && ./scripts/check-all.sh; echo $?` on this container (bun 1.3.11
  present, deno absent), exit code recorded.
- `bun -e '<the extracted regex literal>'` and the same under `node -e`, both outputs recorded.
- Read the CI job after the change and confirm the runtimes it installs match the documented
  claim.

**Gate** — what must pass before approval:

- `cd packages/qfs-viewer && ./scripts/check-all.sh` exits 0, or its non-zero exit is the
  deliberate, documented outcome above.
- `cd packages/qfs && cargo test --workspace` still exits 0 (unchanged, but the repository gate
  stays green).

## Considerations

- Do not "fix" this by deleting bun from the loop. `smoke-npx.sh`'s comments record that a check
  which skipped a runtime silently is exactly how bun stayed broken for a session; a decision to
  stop covering a runtime has to be visible in the output.
- The fix proper is upstream and may take time. What this ticket can settle immediately is the
  second half — that a gate's result must not depend on undocumented properties of the machine
  running it.
- deno is absent from this container, so its leg is unproven here either way; whatever is decided
  for bun should say what deno's status is too.
