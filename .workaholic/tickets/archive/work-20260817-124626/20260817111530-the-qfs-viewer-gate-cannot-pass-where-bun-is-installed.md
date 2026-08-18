---
created_at: 2026-08-17T11:15:30+00:00
status: done
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

## Final Report

Development completed as planned, except step 3 — filing upstream — which this environment cannot do
and which is now its own ticket.

**Step 1 reproduced it and step 2 localized it completely.** `./scripts/smoke-npx.sh` failed exactly as
the ticket describes (node PASS, bun FAIL at the `--version` invocation, deno absent). The failure is a
**parse** error, so it precedes any execution: `bun -e "import('<path>/plgg-md/dist/index.es.js')"` fails
while the same import under node succeeds. Narrowed to one of the four regex literals in the
`src/Render/usecase/slugify.ts` module by extracting each and compiling it in both runtimes:

| Literal | node v22.22.2 | bun 1.3.11 |
| --- | --- | --- |
| `/[̀-ͯ]/g` (combining marks) | OK | OK |
| **`/[<U+0000>-<U+001F>]/g` — raw control bytes** | **OK** | **SyntaxError: range out of order** |
| `/[\s~`!@#$%^&*()…“”‘’-]+/g` | OK | OK |
| `GITHUB_STRIP_RE` (8169 chars) | OK | OK |

**Step 2's question is answered, and the answer is neither of its two options.** The range is **not**
inverted — `0x00 < 0x1F` — so it is not a bundler-emitted inverted range; and it is not a `v`-flag or
unicode-sets difference either. It is bun's **lexer** mishandling raw control characters inside a regex
literal: bun's own error echo prints the literal as `/[`, a line break, then `]/g`, i.e. it read the raw
`0x0A` inside the class as source structure and derived out-of-order endpoints. The decisive control:
the identical class written **escaped** as `/[\0-\x1F]/g` compiles in bun *and* node. (The raw NUL byte
also makes `grep` classify the shipped dist as a binary file — an independent tell.) Confirmed in
**both** `plgg-md@0.0.2` (nested under `plgg-content`) and `@0.0.3` (the declared direct dependency),
so the ticket is right that no bump fixes it: `0.0.3` is the newest published.

**Step 3 is blocked here and was not faked.** `plgg-md` is published from the private `qmu/plgg`
repository, and this run's GitHub access is scoped to `qmu/qfs` — so it cannot open the issue, and it
must not claim an issue number that does not exist. Minted
`20260817131540-file-the-bun-plgg-md-parse-defect-upstream.md`, which carries the whole finding in
paste-ready form (construct, two-runtime table, version scope, the ask) and declares
`verification_handoff:` because its acceptance cannot be verified where an unattended run executes.

### Step 4 — the gate's contract, decided and recorded

Of the ticket's two options, **the named, dated exemption** was taken over "keep failing". Reasoning,
recorded rather than asked:

- `check-all.sh` is the project's canonical runner and every `/implement` tick in a bun-carrying
  container runs it. A permanently red gate on an unchanged tree teaches exactly the "ignore the red
  gate" habit that `smoke-npx.sh`'s own header argues against — and it would make every future
  qfs-viewer ticket report a red gate for a third-party reason.
- Dropping bun from the loop was **not** an option: the script's comments record that a silent skip is
  how bun stayed broken for a whole session, and the ticket's Considerations forbid it.
- The exemption is therefore **loud, dated, tracked, and narrow**. It prints a `NOT COVERED` block, does
  **not** increment `RAN`, states that the published artifact really is broken under bun, carries a
  **revisit-after 2026-11-17** date, and cites the tracking ticket.

**Narrowness is proven, not asserted.** The guard requires the runtime to be `bun` **and** the output to
contain both `range out of order in character class` **and** `plgg-md`. Exercised against five inputs:

| Input | Result |
| --- | --- |
| the real defect (bun + signature + plgg-md) | **EXEMPTED** |
| a *different* bun regex fault in plgg-md (`nothing to repeat`) | FAILS GATE |
| the same signature in a *different* package (`plgg-view`) | FAILS GATE |
| bun failing on our own launcher (`Cannot find module`) | FAILS GATE |
| node hitting the very same signature | FAILS GATE |

### Step 5 — CI coverage and the documented claim now agree

Installing bun in CI would turn `viewer-check-all` red on an unchanged tree for a third-party defect, so
the claim was corrected to match the coverage rather than the reverse:

- `.github/workflows/ci.yml` — the `viewer-check-all` job now states that it installs Node only, that a
  green badge therefore attests to Node alone, and why bun is deliberately absent.
- `packages/qfs-viewer/README.md` — "It runs on node, bun, and deno" kept as the portability intent, now
  followed by a per-runtime status table: node proven, bun broken upstream, deno unproven either way.
- `docs/guide/repository.md` — the same table against the gate, plus why the narrow exemption was
  preferred to dropping bun.

### Changes

- `packages/qfs-viewer/scripts/smoke-npx.sh` — the narrow, dated exemption with the full root cause in
  its comment.
- `.github/workflows/ci.yml`, `packages/qfs-viewer/README.md`, `docs/guide/repository.md` — the runtime
  coverage stated where a reader looks.

### Verification

- `cd packages/qfs-viewer && ./scripts/check-all.sh; echo $?` → **0** on this container (bun 1.3.11
  present, deno absent). The smoke prints: `PASS: node …` / `NOT COVERED: bun …` (with the dated
  exemption and ticket) / `SKIP: deno is not installed`.
- `bun re.js` and `node re.js` over the extracted literal, raw and escaped — the four-cell table above.
- `bun -e "import('…/plgg-md/dist/index.es.js')…"` → `Invalid regular expression: range out of order in
  character class` for **both** 0.0.2 and 0.0.3; `node` → OK for both.
- The five-input narrowness probe above.
- `sh -n scripts/smoke-npx.sh` — syntax clean.
- `cd packages/qfs && cargo test --workspace` — exits 0 (untouched by this change, gate kept green).

### Discovered Insights

- **Insight**: A published bundle can be broken in a way no version bump reaches. `plgg-md` 0.0.2 and
  0.0.3 are the only published versions and **both** carry the defect, so `^0.0.3` already resolves to
  the newest and the dependency range is not the lever. The lever is either an upstream republish or a
  consumer-side exemption — and knowing which *before* editing a manifest saves a pointless bump.
  **Context**: `npm view <pkg> versions` is the cheap check that distinguishes "we are behind" from
  "there is nowhere to go", and it should precede any "try bumping it".
- **Insight**: An emitted bundle carrying **raw control characters** is a portability hazard independent
  of any runtime's bugs — it breaks `grep` (the file reads as binary, which is how the first search for
  the offending literal came back useless) and it exposes lexer differences between engines that
  escaped output never would. `\0-\x1F` and a raw `0x00-0x1F` are semantically identical and only one of
  them is portable.
  **Context**: Worth a bundler-level rule upstream rather than a per-defect fix, since any future raw
  control character in any plgg package would reproduce this class.
- **Insight**: "The gate is environment-sensitive" was already written in `docs/guide/repository.md`
  before this ticket — but only as a principle, with no statement of what CI actually installs. A
  principle without the concrete matrix beside it did not stop a whole session's confusion about whether
  a green CI run meant the runtimes worked.
  **Context**: Same shape as the sibling ticket 20260817110309, where `vitepress dev`'s on-demand
  compilation meant the documented workflow could look healthy over an unbuildable page. Both are the
  local path and the CI path disagreeing about coverage, with nothing saying so.
