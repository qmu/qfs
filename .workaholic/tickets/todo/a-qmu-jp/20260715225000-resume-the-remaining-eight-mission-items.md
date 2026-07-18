---
created_at: 2026-07-15T22:50:00+09:00
author: a@qmu.jp
type: housekeeping
layer: [Config]
effort:
commit_hash:
category:
depends_on:
mission: build-insightbrowser-on-the-plgg-family
---

# Resume: the seven mission items this session could not reach

## CORRECTED — this ticket was written at 9/17 and was wrong twice

Written when I believed bun needed a CLI bundle and RBAC needed a design
decision. Both were reachable, and pushing on them took the mission to
**10/17**. What follows is the state AFTER that, and the corrections matter
more than the list:

- **bun works.** The diagnosis in `20260715223000` was wrong — `tsconfig.json`
  simply did not ship, and a static `plgg-mcp` import pulled `node:sqlite`,
  which bun lacks. Both fixed; that ticket is archived with the correction.
- **RBAC is done.** Principals are declared in `qfs-viewer.config.json`,
  enforced by one middleware over every route, OPEN when none are declared.
- **The mechanism is proven on plgg (~~1711 docs, 249ms~~ **954 docs, 186ms**)
  and qfs (**578**)** without modifying either repository. Running a tool at a
  directory is reading it. **The plgg count was corrected 2026-07-16**:
  `.worktrees` was never pruned, so 44% of it was the same documents at other
  commits, listed beside themselves. `qfs (577)` was right — it had no worktree
  then and has one now, which is why a re-measure read 1154 until the prune
  landed.

**The lesson, recorded because I keep needing it: "impossible" was twice a
diagnosis I had not finished.** Check the constraining thing, not the
constrained one.

## Policies

This ticket routes decisions to a person; it writes no code itself. One policy
governs it, and this ticket is the standing evidence for why:

- `workaholic:planning` / `policies/verify-before-building.md` — **every row in
  the table below is a claim about a wall, and this ticket has been WRONG about
  a wall four times**: bun ("needs a CLI bundle" — no, `tsconfig.json` did not
  ship), RBAC ("needs a design decision" — no, it was buildable), deno ("needs a
  permission", then "one command away" — neither), and WebMCP ("nothing to build
  against" — it is a browser API needing no dependency at all, and Chromium 149
  here implements it). The pattern is identical each time: a real check aimed at
  the constrained thing instead of the constraining one. **Before moving any row
  to a person, re-run its check and paste the output.** A blocker inherited is a
  blocker unverified.
- `workaholic:implementation` / `policies/objective-documentation.md` — when a
  row here is overturned, correct it in place with the measurement attached
  rather than deleting it. The corrections are the most useful thing in this
  file.

## WHAT EACH REMAINING ITEM ACTUALLY NEEDS

Not "hard" — each needs one specific thing, and five of them are a person's
decision rather than a keyboard's:

| item | what it needs |
| --- | --- |
| node/bun/deno | ~~needs a permission~~ ~~installed, needs the check run~~ **DONE 2026-07-16 — the item is checked in `mission.md`.** The smoke prints PASS for node, bun AND deno. It was NOT the one command this row promised: the prediction that "the tsconfig fix covers deno" was wrong (deno reads `deno.json`, and its `register()` is a no-op stub), so the alias moved to package.json `imports` and `bin/hook.mjs` is gone. |
| Voice / Realtime API | **`OPENAI_API_KEY` — still empty, re-verified 2026-07-16. THIS BLOCKER IS REAL** (the only one of the five that was). `env | grep -i openai` matching twice is a FALSE POSITIVE — `PATH` and `CLAUDE_PLUGIN_DATA` both contain the codex plugin's path; use `printenv`. The developer has decided to supply a key: ticket `20260716095636`. But the key was never the whole blocker — three constraints decide the design first: the key must NEVER reach the browser (ephemeral token minted server-side), ADR 0001 forbids the `openai` SDK so it is platform WebSocket/WebRTC or nothing, and voice needs client JS so it inherits ADR 0007 from `20260716025007`. Voice EDITING is a write: it rides `api(ref, {writer})` behind a declared principal, and is unavailable on an OPEN server. |
| Hosted SSR (Worker+D1 / Lambda+EFS) | ~~credentials for either half~~ **NOT BLOCKED — corrected 2026-07-16. Ticket `20260716093913`.** The `NoCredentials` answer was real and asked the **default** profile; a working one sat beside it. `aws sts get-caller-identity --profile q` → PowerUserAccess on account `839625015061` (the developer named and authorised it). The `and/or` means the Lambda half alone satisfies the item, so Cloudflare's absence no longer blocks anything. `toFetch` confirmed as the seam: `(app: Web) => (Request) => Promise<Response>`. |
| qmu.app-adaptive / R2 | ~~Same Cloudflare wall.~~ **MIS-FILED — corrected 2026-07-16.** An account is necessary and NOT sufficient. `FileSystem` (`domain/model/Scan.ts`) is **synchronous** — `readDirectory`/`isDirectory`/`readFile` return values, implemented with `readdirSync`/`statSync`/`readFileSync`. EFS is POSIX so it works unchanged; **R2 and S3 are object stores and cannot be expressed through a sync seam at all.** So this row needs `FileSystem` to become async — reaching `scan`, `reload`, the index and every caller. It is a domain change wearing an infrastructure hat, and handing someone a Cloudflare account would not start it. |
| plgg + qfs docs sites | **Those repositories adopting this**, which is their work. The corpora already serve correctly; a `/request` to each is the sanctioned next step. |
| Browser AI over WebMCP | ~~nothing to build against~~ **WRONG — corrected 2026-07-16. It is REAL, BUILDABLE and VERIFIABLE HERE; ticket `20260716025007` carries it.** Every fact in the old row was true and the conclusion did not follow: WebMCP is not a package, it is a **browser platform API** (`navigator.modelContext.registerTool`), so it needs NO dependency and ADR 0001 never reached it. The npm registry was the wrong place to look. MEASURED: Chromium **149** (`~/.cache/ms-playwright/chromium-1228`) exposes it unflagged; **147** does not (a free negative control). It is a W3C Web ML CG **Draft Community Group Report** of 2026-07-10 — and the spec says `document.modelContext` while the shipped browser says `navigator.` Also corrected: `plgg-server` exports **no** `clientEntry`; it is an optional field on `HtmlDocumentOptions`, paired with the `javascriptResponse` export. |
| Other qfs resources | The largest real piece of work left, and it is unblocked. `qfs` is on PATH. The design question is what a non-markdown resource IS in an index whose whole model is `Document` — a second archetype, or a projection into `Document`? That is a ticket to write, not a patch to make. |

## Quality Gate

### Acceptance Criteria

This ticket ships no code, so its gate is about the HONESTY of the rows, not
about a build:

- Every remaining row carries a **command that was run in this session** and
  its output — not a command that was run once in July and quoted since.
- No row claims a blocker on the strength of the wrong artefact. The test:
  name the thing that would have to change for the row to become false, and
  check THAT. (npm was checked for WebMCP; the browser was the thing.)
- A row overturned is corrected in place, with the measurement, and its ticket
  linked.
- The ticket is only archived when every row is either done or genuinely
  waiting on a person who has been asked.

### Verification Method

```sh
printenv OPENAI_API_KEY                 # Voice: empty = real blocker
aws sts get-caller-identity             # Lambda half of Hosted SSR
command -v wrangler                     # Cloudflare half
```

`env | grep -i openai` is a **known false positive** — `PATH` and
`CLAUDE_PLUGIN_DATA` both contain the codex plugin's path. Use `printenv`.

### Gate

- No row moves to "needs a person" on an inherited claim. Re-run it first.
- Re-read `## Policies` before touching any row: this ticket's four wrong walls
  are the reason that section exists.

## Implementation Steps

1. **Take the four remaining decision items to the developer.** Credentials for
   two, a scope decision for the docs sites. (WebMCP is no longer one of them —
   it is `20260716025007`.)
2. **`20260715223100`** (plgg-mcp's unconditional `plgg-content` import) is a
   `/request` away and makes `qfs-viewer mcp` cross-runtime.
3. **qfs resources** is the one substantial thing a session can start cold.
   Write it as a `/ticket` first — the model question above decides everything
   downstream.

## Considerations

- **ADR 0005's first retirement date is 2026-07-22 21:09 JST** — the
  `NPM_CONFIG_MIN_RELEASE_AGE=0` override in `scripts/smoke-npx.sh`. It has
  moved once already; the ADR says when to stop extending.
- **`.workaholic/stories/work-20260715-003158.md` still does not parse** —
  `/report`'s story format writes a multi-line flow sequence, and plgg-md takes
  flow sequences on one line only. It is served; only its front matter is
  `None`.
- **Live surfaces to check anything against**: `/` (columns), `/<path>`,
  `/edit/<path>`, `/api/*`, `qfs-viewer mcp` over stdio, and any of them
  with `Authorization: Bearer <key>` once principals are declared.

## Overview

**Carry origin:** the 2026-07-15 `/drive` + `/goal` session, which took the
mission from 2/17 to **9/17** in 16 commits on `work-20260715-172000`.
`./scripts/check-all.sh` exits 0; 223 tests; coverage >90 on all four metrics.

**Read this before picking anything up:** five of the eight remaining items are
blocked on things no amount of coding fixes, and one was blocked by a
permission refusal. Sorting them honestly is the point of this ticket — a
session that starts at the top of the list will waste an hour discovering the
same walls.

## Findings

### Done this session (do NOT redo)

Scanner + front-matter index, SSR with heading auto numbering, the
column-accretion UI (**the mission's gate — met live**), tag groups, the REST
query surface, the MCP server, in-browser editing, and
`qfs-viewer.config.json`. The mission's changelog carries the decisions and
the bugs each one surfaced.

### CANNOT be done from this environment (5 items)

Not "hard" — **impossible**, and each was checked rather than assumed:

- **Voice Q&A / Realtime API** — `OPENAI_API_KEY` is not set. Nothing to
  point at.
- **Hosted SSR (Cloudflare Worker + D1, Lambda + EFS + sqlite)** — no
  `wrangler`, no Cloudflare account. `aws` is on PATH but no target exists.
- **qmu.app-adaptive / R2 offload** — same Cloudflare wall.
- **plgg and qfs documentation sites** — both live in OTHER repositories
  (`/home/ec2-user/projects/plgg`, `/home/ec2-user/projects/qfs`). Only
  `/request` may cross that boundary, and building a site there is that repo's
  work, not a request.
- **Other qfs resources browsable** — same boundary.

These need a developer with credentials, or a decision to move them out of this
mission. **Do not fake a gate for them.**

### ~~BLOCKED on a permission, not a design (1 item)~~ — DONE 2026-07-16

- ~~**`npx qfs-viewer` on node, bun, and deno.**~~ **MET.** Checked in
  `mission.md`; the smoke packs, installs and RUNS the bin under all three.

  Worth keeping, because this row was wrong twice in the same direction. It
  first said the item needed a permission; then, once deno was installed, that
  it was "one command from being verifiable" because the tsconfig fix *should*
  cover deno "for the same reason it covered bun". Both were predictions
  standing in for a run. deno reads `deno.json`, not `tsconfig.json`, and its
  `register()` is a silent no-op stub — so deno had no resolver at all. The
  real cause was one fact declared three times (hook.mjs / tsconfig `paths` /
  nothing), which is why "it resolves" was true once per runtime. It is now
  declared once, in package.json's `imports`.

### REACHABLE, and the one real piece of work left (1 item)

- **RBAC and principal management: users and bots (API-key issued), enforced
  over read and edit.**

  **`plgg-auth` is the wrong tool, checked not guessed.** It is an OIDC
  identity-provider toolkit, and it pulls:
  - `plgg-server ^0.0.3` while this repo is on `^0.0.4` — the exact dependency
    diamond that cost this session an hour (see commit `1213d94`); a caret on a
    `0.0.x` version is an exact pin, so npm would install both and the `Html`
    types would stop unifying again.
  - `plgg-sql` + `plgg-db-migration` — **a database**, which the mission's own
    Goal rules out for the local surface: "no build step, no database, no
    central configuration".

  So the shape to build is almost certainly: **principals declared in
  `qfs-viewer.config.json`** (which now exists and validates), an API key
  or bearer token mapping to a principal, and enforcement at the two seams that
  already exist for it — the `edit` capability is ALREADY an argument
  (`api(ref, {fs, writer})`), so a read-only server is read-only by
  construction rather than by a check. That is the hook RBAC should hang on.

  Note the MCP surface is deliberately read-only until this lands
  (`entrypoints/mcpTools.ts` says why): an MCP tool that writes before
  principals exist is an unauthenticated write to someone's working tree.

## Implementation Steps

1. **Do the two defect tickets first** — they are small and they unblock
   honesty about the product: `20260715223000` (bun/deno) and `20260715223100`
   (the SQLite warning `plgg-mcp` drags in; upstream-only, `/request` it).
2. **RBAC**, on the shape above. Design it as a ticket first — it is the last
   substantial item and it touches every surface.
3. **Take the five blocked items to the developer**, not to a keyboard. They
   need credentials or a mission-level decision about scope.

## Considerations

- **ADR 0005's retirement schedule is live and its first date has moved to
  2026-07-22 21:09 JST** — the `NPM_CONFIG_MIN_RELEASE_AGE=0` override in
  `scripts/smoke-npx.sh`. Every upstream fix adopted pushes that date out
  another week; the ADR says when to stop extending rather than keep bumping it.
- **One document still fails to parse**: `.workaholic/stories/work-20260715-003158.md`,
  because `/report`'s story format writes a MULTI-LINE flow sequence and
  plgg-md's subset takes flow sequences on one line only. It is served; only
  its front matter is `None`. Worth an upstream request if `/report` keeps
  emitting that shape.
- **The live surfaces to sanity-check anything against**: `/` (columns),
  `/<path>` (rendered document), `/edit/<path>`, `/api/*`, and
  `qfs-viewer mcp` over stdio.
