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
