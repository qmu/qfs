---
created_at: 2026-07-16T09:56:36+09:00
author: a@qmu.jp
type: enhancement
layer: [UX, Infrastructure]
effort: 4h
commit_hash:
category: Added
depends_on: [20260716025007-webmcp-tools-over-the-live-corpus.md]
mission: build-insightbrowser-on-the-plgg-family
---

# Voice Q&A and voice editing over the Realtime API

## The credential blocker is REAL — and it is not the interesting one

Unlike the four walls that fell before it, this one is genuine and re-verified
on 2026-07-16:

```sh
$ printenv OPENAI_API_KEY     # empty
```

Note `env | grep -i openai` matches TWICE and is a **false positive** — `PATH`
and `CLAUDE_PLUGIN_DATA` both contain the codex plugin's path. Use `printenv`.

**The key was supplied on 2026-07-16 and lives in `.env` at the repo root** —
which is this repository's established credential contract, not an ad-hoc
choice. `scripts/serve-development.sh:15-18` states it:

> Credentials come from ONE git-ignored `.env` at the repo root: its
> `KEY=value` lines are exported here so a workload's `${VAR:-}` compose
> interpolation picks them up […] the caller's environment wins.

Verified: `.env` is ignored at `.gitignore:26` (`.env` and `.env.*`, with
`!.env.example`) and is **not tracked**. `OPENAI_API_KEY` is defined in it,
alongside `WORKAHOLIC_PORT_BASE`, `WORKAHOLIC_DEV_PORT`, `WORKAHOLIC_DOCS_PORT`.

**Consequence to plan for: nothing auto-loads `.env`.** A bare `printenv
OPENAI_API_KEY` in any shell — including a `/drive` session's — is EMPTY, and
that is not evidence the key is missing. It reaches a process only via
`scripts/serve-development.sh`, which exports it before starting the workload.
So the voice feature is configured **through the workload**, and a session
verifying it must go through that script rather than expect the ambient
environment to carry it.

(An earlier draft of this ticket said the key belonged in
`.claude/settings.local.json`. That was invented rather than checked — the
house already had a contract, and `serve-development.sh` is where it is
written down.)

**Do not paste the key into a ticket, a commit, a test, or the chat.** The
release scan treats a pasted token as a credential finding whether or not it is
real, and it is right to (see commit `3353626`: fake bearer tokens in
`edit.spec.ts` tripped it, and the scan was correct).

## THREE constraints that decide the design before any code

### 1. The key must NEVER reach the browser

Voice means a microphone, which means the browser. The naive shape — browser
talks to `api.openai.com` with `OPENAI_API_KEY` — **ships the key to every
visitor**, and `insight-browser.qmu.dev` is published (behind Access, which is
not the same as private).

So the server mints a **short-lived ephemeral session token** and the browser
uses only that. The key stays server-side. This is not a nicety; it is the
difference between a feature and a credential leak, and it is why this item is
`Infrastructure` and not only `UX`.

### 2. There can be no `openai` SDK — ADR 0001 forbids it

CLAUDE.md: *"No non-plgg dependencies."* ADR 0001 makes the dependency contract
a gate (`scripts/gate-dependencies.sh` **fails the build** on a foreign runtime
dep, and it self-tests). So the official SDK is not available, and that is not
a problem to route around — it is the same shape as WebMCP: **use the
platform.** The Realtime API is reachable over WebSocket/WebRTC, both of which
the browser provides natively, and the token mint is one `fetch` from the
server. Zero dependencies, exactly like the rest of this product.

### 3. It needs client JS — so it INHERITS the ADR that WebMCP forces

`entrypoints/columns.ts:7` says this product ships no client JavaScript, and
that is what makes "every column is a function of the URL" true. Voice cannot
be done server-side. **`depends_on` is set accordingly**: ticket
`20260716025007` writes ADR 0007 on the client-JS stance, and this item is
downstream of whatever it decides. Do not re-litigate it here; if that ADR says
"additive and non-rendering only", then a voice UI that re-renders columns is
out of bounds and the answer must arrive as a URL, not as DOM.

## Voice EDITING is a WRITE, and writes have a story here

The item is "voice Q&A **and voice editing**". The second half is the sharp
one, and this repository has already reasoned about it:

- `entrypoints/mcpTools.ts` is deliberately **read-only**, and says why: a tool
  that writes before principals exist is an unauthenticated write to someone's
  working tree.
- **Principals now exist** (`domain/model/Principal.ts`, `asPrincipal`), so
  that reasoning has moved — but it has not evaporated. RBAC is declared in
  `qfs-viewer.config.json` and is **OPEN when nothing is declared**, which
  is the `npx` case: your own repo, your own machine. Voice editing on an OPEN
  server is a microphone with commit rights.
- The edit capability is **already an argument** (`api(ref, { fs, writer })`),
  so a read-only server is read-only by construction rather than by a check.
  Voice editing must ride that seam, not bypass it.

So: **Q&A and editing are two deliverables, not one.** Q&A is read-only and
lands first. Editing lands only behind a declared principal, and the ADR/ticket
must say what happens on an OPEN server — most likely: voice editing is
unavailable unless principals are declared.

## Policies

- `workaholic:design` / `policies/data-sovereignty.md` — **the question under
  the credential.** Voice Q&A streams this corpus to a third party. The corpus
  here is `.workaholic/` — tickets, stories, mission files — and this project
  has `/request` precisely BECAUSE crossing a repository boundary needs
  customer context masked and confirmed first. Streaming the same content to
  `api.openai.com` is a boundary crossing with no mask. State what may leave,
  and what must not, before this is switched on for any corpus but this one.
- `workaholic:design` / `policies/access-control.md` — the ephemeral token, and
  voice editing behind principals. An OPEN server plus a microphone is the
  configuration to think hardest about.
- `workaholic:design` / `policies/auth-procurement.md` — the token-mint
  endpoint is an auth surface this product does not yet have; it must not
  become a second, weaker one beside the principal middleware.
- `workaholic:planning` / `policies/verify-before-building.md` — this blocker is
  real, and that is exactly when to re-check rather than relax: re-run
  `printenv OPENAI_API_KEY` at implementation time. Four rows on this branch
  were "blocked" on checks that asked the wrong thing.
- `workaholic:planning` / `policies/legal-compliance-check.md` — a paid
  third-party API processing repository content in a corporate context is a
  contractual question as much as a technical one.
- `workaholic:implementation` / `policies/directory-structure.md` — universal.
- `workaholic:implementation` / `policies/coding-standards.md` — universal. The
  Realtime API's messages arrive as `unknown` at the boundary and get parsed,
  not asserted. No `as` to make a stream event fit a hoped-for shape.
- `workaholic:implementation` / `policies/anti-corruption-structure.md` — the
  token mint and the voice entry are entry points over the same index; the
  answer comes from the same procedures `/api/*` already calls.
- `workaholic:implementation` / `policies/objective-documentation.md` — the
  ephemeral-token design and the OPEN-server editing rule are non-obvious
  decisions with alternatives; they need the reasoning recorded.

## Key Files

- `src/entrypoints/api.ts:183` — `writer: FileWriter` as an argument; the seam
  voice editing must ride.
- `src/domain/model/Principal.ts:48,93` — `Principal`, `asPrincipal`. Note
  `asPrincipal` REFUSES a key under 16 characters (a short key is not access
  control but the appearance of it).
- `src/entrypoints/mcpTools.ts` — the read-only stance and its stated reason.
- `src/entrypoints/columns.ts:7` — the no-client-JS stance this inherits.
- `scripts/serve-development.sh:15-37` — the credential contract: the ONE
  git-ignored `.env` at the repo root, exported for the workload's `${VAR:-}`
  compose interpolation. This is how the key reaches the running product, and
  the only way it does.
- `.env` — where the key IS (gitignored `.gitignore:26`, untracked). Never read
  its value into a log, a test, a commit, or a tool call: even a prefix is
  exposure.
- `.env.example` — the committed contract (`.gitignore:28` un-ignores it
  deliberately). Names and placeholders only.

## Implementation Steps

1. **Wait for ADR 0007** (`20260716025007`). If client JS is refused, this item
   is refused with it and the mission text needs correcting — say so rather
   than building around the decision.
2. **Q&A first, read-only.** Server endpoint mints an ephemeral Realtime
   session token; the browser opens WebRTC/WebSocket with that token only.
3. Ground the answers in the corpus via the tools that already exist — the same
   four `mcpTools.ts` registers. If `20260716025007` has landed, WebMCP has
   already exposed them to the page and voice is a consumer, not a second
   integration. **One tool registry, not two.**
4. **Voice editing second, and only behind a declared principal**, through
   `api(ref, { fs, writer })`. On an OPEN server it is unavailable, by
   construction rather than by a check.

## Quality Gate

### Acceptance Criteria

- `OPENAI_API_KEY` never appears in any response body, any served asset, any
  log line, or any committed file. **Grep the served page for it.**
- The browser holds only an ephemeral token, and it expires.
- No non-plgg dependency is added — `scripts/gate-dependencies.sh` stays green.
- Voice Q&A answers a question about the live corpus, and the answer matches
  what `/api/*` returns for the same question.
- Voice editing is UNAVAILABLE on a server with no principals declared, and
  writes as the declared principal when one is.
- With JS off, the page is exactly what it is today (inherited from ADR 0007).

### Verification Method

```sh
# The key is defined in .env and NOTHING auto-loads it. Do not `printenv` it
# and conclude it is missing -- that is what serve-development.sh is for, and
# never print any part of the value: a prefix is still a credential.
grep -q '^OPENAI_API_KEY=' .env && echo "configured"   # presence, not value

./scripts/serve-development.sh                      # exports .env, boots workload

curl -sf localhost:4100/ | grep -c 'sk-'                # MUST be 0
curl -sf localhost:4100/<voice-asset> | grep -c 'sk-'   # MUST be 0
git grep -c 'sk-' -- . ':!*.md' | head               # MUST find nothing
./scripts/gate-dependencies.sh                      # no foreign dep crept in
./scripts/check-all.sh                              # exits 0
```

Voice itself needs a microphone, so the end-to-end is **developer-run, not
CI-run**. Say that out loud in the deployment notes rather than pretending the
gate covers it.

### Gate

- The key-leak greps above are **hard blockers**, not advisories.
- `./scripts/check-all.sh` exits 0.
- The mission item is checked only after a real voice exchange against the live
  corpus. Writing the plumbing and never speaking to it is a claim, not a gate
  — the rule the deno row learned the hard way.

## Considerations

- **This is the only mission item requiring a paid third-party API and an
  outbound data flow**, in a product whose pitch is "npx at a repository root —
  no build step, no database, no central configuration". That tension is worth
  naming in the ADR: the feature is fine, but it must be OPTIONAL and OFF by
  default, or the product's own claim stops being true for anyone who runs it.
- **`qfs-viewer.config.json` is the natural switch** — it already exists,
  validates, and declares principals. Voice belongs there: absent means off.
- My knowledge of the Realtime API predates July 2026 and the WebMCP lesson
  applies — **read the current API docs before designing the token mint.**
  Every "impossible" and every "obvious" on this branch that came from memory
  rather than measurement has been wrong.
