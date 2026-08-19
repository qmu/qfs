---
created_at: 2026-08-17T13:15:40+00:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission: the-current-situation-of-qfs-is-documented-as-it-actually-stands
merge_policy:
verification_handoff: Filing an issue in the private qmu/plgg repository — an unattended run's GitHub access is scoped to qmu/qfs, so it cannot open, comment on, or verify an issue there.
---

# File the bun/plgg-md parse defect upstream

## Overview

Minted while driving `20260817111530-the-qfs-viewer-gate-cannot-pass-where-bun-is-installed.md`, which
localized the defect completely but **could not file it**: this run's GitHub access is scoped to
`qmu/qfs`, and `plgg-md` is published from the private `qmu/plgg` repository. Step 3 of that ticket —
"File it upstream against `plgg-md`, with the extracted literal and both runtimes' behaviour" — is the
one part that needs an account with reach into `qmu/plgg`.

Everything the upstream report needs is already established, so this ticket is the filing, not the
investigation.

## The finding, ready to paste upstream

`plgg-md`'s published dist cannot be parsed by bun. The failure is at parse time, before any consumer
code runs, so `npx`-installing anything that depends on `plgg-md` is broken under bun:

```
SyntaxError: Invalid regular expression: range out of order in character class
  at <parse> (…/node_modules/plgg-md/dist/index.es.js:1124:1)
Bun v1.3.11 (Linux x64)
```

**The construct.** One regex literal in the `src/Render/usecase/slugify.ts` module of
`dist/index.es.js` (and `dist/index.cjs.js`) is emitted with **raw, unescaped control characters**:

```js
/[<U+0000>-<U+001F>]/g      // the two class endpoints are literal 0x00 and 0x1F bytes
```

The range is **not** inverted — `0x00 < 0x1F` — and the class is valid ECMAScript. Written with escapes
it is accepted by both runtimes:

| Form | node v22.22.2 | bun 1.3.11 |
| --- | --- | --- |
| `/[<0x00>-<0x1F>]/g` (raw bytes, as published) | compiles | **SyntaxError: range out of order** |
| `/[\0-\x1F]/g` (escaped, same semantics) | compiles | compiles |

So bun's lexer mis-consumes the raw control bytes inside the literal and derives out-of-order endpoints.
Its own error echo shows this — it prints the literal as `/[` then a line break then `]/g`, i.e. it read
the raw `0x0A` in the range as source structure. The NUL byte also makes `grep` classify the shipped dist
as a binary file, which is a useful independent tell.

**Scope.** Present in `plgg-md@0.0.2` **and** `@0.0.3`. `npm view plgg-md versions` →
`["0.0.2","0.0.3"]`, so `0.0.3` is the newest and no version bump fixes it. The other three regex
literals in the same module (the combining-mark class, the punctuation class, and the 8169-character
`GITHUB_STRIP_RE`) all compile in both runtimes — only the raw-control-character one fails.

**The ask.** Escape control characters in the bundler's regex output — `\0-\x1F` rather than raw bytes.
That is a build-side fix in `qmu/plgg`, needs no source semantics change, and unblocks bun for every
plgg-family consumer at once. (It is also arguably a bun bug worth reporting to bun, but the escape is
the fix that ships.)

## Policies

- `workaholic:operation` / `policies/ci-cd.md` — a known defect needs a tracked owner, not a comment
- `workaholic:design` / `policies/vendor-neutrality.md` — the runtime matrix is a product promise, so a
  runtime the published artifact cannot run is a product defect and not merely a gate inconvenience
- `workaholic:implementation` / `policies/objective-documentation.md` — the exemption in
  `smoke-npx.sh` cites this ticket, so this ticket must stay resolvable

## Key Files

- `packages/qfs-viewer/scripts/smoke-npx.sh` — carries the dated exemption that cites this ticket by
  number. When the upstream fix lands, the exemption is what gets removed.
- `packages/qfs-viewer/packages/qfs-viewer/package.json` — declares `plgg-md: ^0.0.3`; a republished
  patch resolves without an edit here.
- `packages/qfs-viewer/README.md`, `docs/guide/repository.md` — both now state that bun is not covered
  and why; both need updating when it is.

## Implementation Steps

1. Open the issue in `qmu/plgg` with the section above (the construct, the two-runtime table, the
   version scope, the ask). Requires an account with access to that repository.
2. Record the issue number back into `packages/qfs-viewer/scripts/smoke-npx.sh`'s exemption comment,
   replacing the reference to this ticket with the upstream issue URL.
3. When a fixed `plgg-md` is published: bump/resolve it, remove the exemption from `smoke-npx.sh`, and
   confirm `./scripts/check-all.sh` passes with bun present and **no** NOT-COVERED line.
4. Update `packages/qfs-viewer/README.md` and `docs/guide/repository.md` to drop the bun caveat, and
   consider adding bun (and deno) to CI's `viewer-check-all` job now that the matrix genuinely passes.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- An issue exists in `qmu/plgg` carrying the construct, both runtimes' behaviour, and the affected
  versions.
- `smoke-npx.sh`'s exemption cites that issue rather than this ticket.
- When the upstream fix ships: the exemption is gone, and `check-all.sh` exits 0 on a bun machine with
  no NOT-COVERED line for bun.

**Verification method** — the commands/tests/probes that prove them:

- The issue URL, pasted into this ticket's Final Report.
- `grep -n 'qmu/plgg' packages/qfs-viewer/scripts/smoke-npx.sh` resolves to the issue URL.
- `bun -e "import('<path>/plgg-md/dist/index.es.js').then(()=>console.log('OK')).catch(e=>console.log(e.message))"`
  prints `OK` against the fixed version — the exact probe that isolated the defect.
- `cd packages/qfs-viewer && ./scripts/check-all.sh; echo $?` on a machine with bun installed → 0.

**Gate** — what must pass before approval:

- `cd packages/qfs-viewer && ./scripts/check-all.sh` exits 0.

## Considerations

- **Steps 3 and 4 are blocked on someone else publishing a package**, so this ticket may sit for a
  while after step 1 is done. Splitting it — file now, adopt later — is reasonable if the wait is long;
  the exemption's own 2026-11-17 revisit date is the backstop either way.
- `verification_handoff` is set because the acceptance cannot be verified where an unattended run
  executes: it needs reach into a private repository this runner is not scoped to. That is a property of
  the environment, not of the work.
- deno is **absent** from the container this was found on, so its leg of the matrix is unproven rather
  than broken. If deno turns out to hit the same parse path, this one upstream fix covers it too — worth
  checking on a machine that has deno before assuming deno is fine.

## Blocked — 2026-08-17 (work-20260817-132305)

**Blocker: this session cannot write to `qmu/plgg`, so step 1 cannot be performed.** The ticket stays
in `todo/` and is claimable the moment someone with reach into that repository picks it up. Nothing
about the finding is outstanding — the report body above is complete and paste-ready.

Two concrete facts, not a forecast:

1. **The session's GitHub access is scoped to `qmu/qfs` alone**, stated in the runner's own
   configuration: "GitHub access for this session is currently scoped to: `qmu/qfs` … Do NOT read
   from, write to, or search across any repository not listed above — calls targeting them will be
   denied." A cross-repository write was therefore not attempted rather than attempted and refused:
   issuing a call the runner is instructed not to make is not evidence-gathering.
2. **Writes through this proxy are gated, observed directly this run.** Deleting the previous unit's
   merged claim branch by REST returned, verbatim:

   ```
   {"message":"Write access to this GitHub API path is not permitted through this proxy.",
    "documentation_url":"https://docs.anthropic.com/en/docs/claude-code/github-actions"}
   gh: Write access to this GitHub API path is not permitted through this proxy. (HTTP 403)
   ```

   That was a write to an **in-scope** repository. A write to an out-of-scope one has strictly less
   chance of succeeding.

**What unblocks it:** a session or person authenticated with write access to `qmu/plgg`. That is the
same condition this ticket's `verification_handoff:` already declares, which is why the declaration
was recorded at creation rather than discovered here.

**Unaffected by the block:** the consumer-side exemption in `packages/qfs-viewer/scripts/smoke-npx.sh`
is already in place and cites this ticket, so the gate is green and the defect is visible in the
smoke's output meanwhile. Its 2026-11-17 revisit date is the backstop if this ticket is not picked up.

## Still blocked — 2026-08-18 (work-20260818-224038)

Re-checked by an unattended `/implement` run. The blocker is unchanged and no part of the ticket
became runnable; recorded here so the wait is dated rather than assumed.

What was actually run this time:

| Check | Command | Result |
| --- | --- | --- |
| Has a fixed `plgg-md` been published? | `npm view plgg-md versions --json` | `["0.0.1","0.0.2","0.0.3"]`, `dist-tags.latest = 0.0.3` — no new release, so steps 3 and 4 stay blocked on the upstream publish |
| Is deno available to close the matrix gap named in Considerations? | `which deno` | not found (`bun` 1.3.11 and `node` present) — deno's leg stays unproven, not broken |
| Can this runner reach `qmu/plgg`? | not attempted | the session's stated scope is still `qmu/qfs` alone; issuing a call the runner is instructed not to make is not evidence-gathering (unchanged from the 2026-08-17 entry) |

So step 1 still needs a person or session with write access to `qmu/plgg`, and steps 2-4 still
follow from it. Nothing in the paste-ready report body above has gone stale: `0.0.3` is still the
newest published version, so the "no version bump fixes it" claim and the `smoke-npx.sh` exemption
that cites it both remain true as written.

## Final Report — 2026-08-19, resolved (all four steps)

Picked up by a developer-attended session on a host that has **node v24.13.1, bun 1.3.14 and deno
2.9.2** — the three runtimes the product promises, none of which the unattended container had
together. That is what unblocked it, and it turned the ticket over completely rather than by half.

### Step 1 — the upstream issue exists

Filed as **[qmu/plgg#131](https://github.com/qmu/plgg/issues/131)** on 2026-08-18T22:47:43Z,
carrying the construct, the two-runtime table and the affected versions, exactly as the paste-ready
body above specifies. `qmu/plgg` is a **public** repository, not private as this ticket's
`verification_handoff:` and its two blocked entries assumed — that assumption is the only thing in
the ticket that was wrong, and it is worth remembering because it kept three runs from checking.

### Step 2 — the exemption cites the issue

`packages/qfs-viewer/scripts/smoke-npx.sh` no longer points at this ticket file:
`grep -n 'qmu/plgg' packages/qfs-viewer/scripts/smoke-npx.sh` resolves to the issue URL.

### Steps 3 and 4 — the defect was bun's, and bun fixed it

The wait was mis-modelled. Both remaining steps were recorded as blocked on *someone publishing a
fixed `plgg-md`*, but the exemption's other trigger — "as soon as a bun release parses the published
dist" — is the one that fired. Measured here against the **unchanged** `plgg-md@0.0.3` tarball
(`npm view plgg-md dist-tags.latest` → `0.0.3`; its dist still carries the literal `0x00` byte and
still emits `/[<0x00>-<0x1F>]/g` with raw endpoints at offset 61336):

| Runtime | `import()` of `plgg-md/dist/index.es.js` |
| --- | --- |
| bun 1.3.11 | `SyntaxError: Invalid regular expression: range out of order in character class` |
| bun 1.3.12 | same `SyntaxError` |
| **bun 1.3.13** | **parses** |
| bun 1.3.14 | parses |
| node v24.13.1 | parses |
| deno 2.9.2 | parses |

So the fault was bun's lexer, bun shipped the fix in **1.3.13**, and the package never had to move.
Each version was probed with its own official release binary, not inferred from a changelog.

What that made possible:

- **The exemption is gone.** In its place `smoke-npx.sh` carries the measured version floor: the
  same narrow signature match now **fails** with "fixed in bun 1.3.13 — upgrade" instead of
  reporting `NOT COVERED`, because an old local bun is a fixable local condition, not an upstream
  wait. bun counts in `RAN` again.
- **The gate passes with all three runtimes present, no `NOT COVERED` line.**
  `cd packages/qfs-viewer && ./scripts/check-all.sh` → exit **0**, and its smoke section reads
  `PASS: node …`, `PASS: bun …`, `PASS: deno …`, with `grep -ci 'NOT COVERED'` → `0`.
- **CI exercises the real matrix.** `viewer-check-all` installs bun (`oven-sh/setup-bun@v2`) and
  deno (`denoland/setup-deno@v2`) alongside Node 24, so a green badge stops attesting to node alone.
- **Both caveats are retired**, in `packages/qfs-viewer/README.md` and `docs/guide/repository.md`:
  bun is proven from 1.3.13, deno is proven against 2.9.2, and the history is kept as history rather
  than deleted, so a reader meeting an old bun still finds the explanation.

### What is left, and where it lives

`qmu/plgg#131` stays **open** on purpose. Emitting the class endpoints as `\0-\x1F` instead of raw
bytes is still the correct build-side fix — the raw NUL also makes `grep` treat the shipped dist as
binary — but it is now build hygiene in another repository, and **nothing in `qmu/qfs` waits on
it**. The 2026-11-17 revisit date that backstopped the exemption is retired with the exemption.

### Verification

| Claim | Command | Result |
| --- | --- | --- |
| No fixed `plgg-md` was published | `npm view plgg-md versions --json`, `npm view plgg-md dist-tags.latest` | `["0.0.1","0.0.2","0.0.3"]`, `0.0.3` — unchanged, so the runtime is what moved |
| The raw-byte class is still shipped | byte scan of `node_modules/plgg-md/dist/index.es.js` | NUL present; `[\x00-\x1f]` at offset 61336 |
| bun's fix landed in 1.3.13 | official `bun-linux-aarch64` release binaries for 1.3.11/1.3.12/1.3.13, each running the `import()` probe | `SyntaxError`, `SyntaxError`, `OK` |
| The exemption cites the issue | `grep -n 'qmu/plgg' packages/qfs-viewer/scripts/smoke-npx.sh` | the issue URL, twice (comment and operator-facing output) |
| The gate is green with no exemption | `cd packages/qfs-viewer && ./scripts/check-all.sh; echo $?` | `0`; `PASS` for node, bun and deno; `NOT COVERED` count `0` |

