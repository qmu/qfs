---
created_at: 2026-07-15T01:49:49+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure, Domain]
effort: 1h
commit_hash:
category: Changed
depends_on: [20260715004235-markdown-scanner-and-frontmatter-index.md]
mission: build-insightbrowser-on-the-plgg-family
---

# REST API serving the indexed model

## Overview

Serve the corpus over HTTP as JSON: the second surface over the one model the scanner built. After this ticket, a program can `curl` the same documents a person will later read as SSR HTML and an agent will query over MCP.

**Why this ticket exists now.** It is the mission's largest acceptance item that is **not** blocked by the release-age policy. `plgg-server@0.0.3` is consumable today, and its dependency closure (`plgg ^0.0.27`, `plgg-http ^0.0.2`, `plgg-view ^0.0.1`) is entirely plgg-family and entirely consumable — verified against the registry. Crucially, a REST surface over document paths, sources, and scan errors needs **no front-matter parsing**, so it is unaffected by the plgg-md 0.0.2 wait that blocks tickets 2 and 3.

It also buys something structural. `anti-corruption-structure` says the evidence the domain/entrypoint separation held is that *a second entry point can start the same domain procedure identically*. Right now that is a claim. This ticket is the first chance to make it a fact — and to find out whether tonight's `Index`/`IndexRef` design actually survives contact with a second consumer, while the design is a day old rather than a month.

**Tag filtering is deliberately out of scope** — it needs the front matter that ticket 2 is blocked on. The endpoints here are the ones that stand on `path` + `source` alone.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — applies to all code work.
- `workaholic:implementation` / `policies/coding-standards.md` — applies to all code work; no `any`/`as`/`!`/`@ts-ignore`.
- `workaholic:implementation` / `policies/anti-corruption-structure.md` — **the governing policy**. The router is a thin `entrypoints/` shell calling the domain's public procedures and formatting the result. It holds no domain logic; that both it and the CLI start the same `scan`/`Index` procedures is the evidence the separation held.
- `workaholic:design` / `policies/modular-monolith-first.md` — REST is a surface of the same deployment unit, not a service.
- `workaholic:design` / `policies/vendor-neutrality.md` — the handlers are written against plgg-http's `HttpRequest`/`HttpResponse`, never a platform `Request`/`Response`. `toFetch` is the only seam that touches platform types, which is what keeps the Worker/Lambda adapter an IO conversion later.
- `workaholic:implementation` / `policies/type-driven-design.md` — a request path is user input: it arrives as `unknown`/text and passes `asDocumentPath` before it can address the index.
- `docs/adr/0003-no-caching.md` — **every** response carries `cache-control: no-store, must-revalidate`. A stale document is an incident.
- `workaholic:implementation` / `policies/test.md` — drive the real `Web` through `handle`, asserting on rendered state.

## Key Files

Reference (do **not** edit):

- `/home/ec2-user/projects/plgg/packages/plgg-server/src/Routing/model/Web.d.ts` (published 0.0.3) - `web()`, data-last `get(path, handler)`, `route()` for mounting a sub-app under a base path with scoped middleware. `Handler = (c: Context) => PromisedResult<HttpResponse, HttpError>`.
- `plgg-server/dist/Routing/usecase/handle.d.ts` - `handle(app, request): PromisedResult<HttpResponse, HttpError>` — the plgg-native entry, no platform types. **This is what the tests drive.**
- `plgg-server/dist/Routing/usecase/toFetch.d.ts` - `toFetch(app): Fetch` — the only place platform `Request`/`Response` surface.
- `plgg-server/dist/{node,bun,deno}.d.ts` - `serve(options, onListen?)(handler)` — three runtimes, already shipped. Relevant to the mission's node/bun/deno item (blocked separately: neither bun nor deno is installed here).
- `plgg-http/dist/...` - `jsonResponse(data, status?, headers?)`, `notFound(path): HttpError`, `httpErrorToResponse`.

Target (this repository):

- `packages/insightbrowser/src/entrypoints/api.ts` - the router (new).
- `packages/insightbrowser/src/entrypoints/cli.ts` - gains the `serve` verb.
- `packages/insightbrowser/src/domain/usecase/scan.ts`, `domain/model/Index.ts` - read, not changed: the API must fit the domain as it stands, or the misfit is the finding.

## Related History

`20260715004234` established the layout and the gates; `20260715004235` built the `Index` and `IndexRef` this ticket reads. The `IndexRef` doc comment claims a reader "cannot observe a torn index" — this ticket is the first real reader, so that claim gets exercised rather than asserted.

## Implementation Steps

1. **Add `plgg-server ^0.0.3` to `dependencies`.** The gate permits it (plgg-family); its closure adds `plgg-http`/`plgg-view` transitively, no third party.
2. **Write the routes** (`entrypoints/api.ts`) on `pipe(web(), get(...), ...)`:
   - `GET /api/documents` → `{ documents: [{ path }], count }`, path-ordered.
   - `GET /api/documents/*path` → `{ path, source }`, or 404 via `notFound`.
   - `GET /api/errors` → `{ errors: [{ path, message }] }` — the corpus's failures are part of the model, not a log line.
   - `GET /api/health` → `{ documentCount, errorCount }`.
3. **Validate the path**: the wildcard capture is user input — `asDocumentPath` it before touching the index. A traversing or absolute path is a 404, not a read. (The brand makes this hard to skip: `getDocument` will not accept a bare string.)
4. **Read the index once per request** via `IndexRef.current()`, then work with that value — the whole point of the swap design.
5. **Set `cache-control: no-store, must-revalidate` on every response**, including 404s, in one place rather than per handler.
6. **Add the `serve` verb** to the CLI: scan, hold the index in an `IndexRef`, serve on `--port` (default 4100, the mission's gate port).
7. **Test** by driving `handle(app, request)` directly: list, fetch, 404 on absent, 404 on traversal, errors surfaced, no-store present, and a read across a `swap` seeing a consistent index.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- `GET /api/documents` returns 200 and every scanned document, path-ordered.
- `GET /api/documents/<known>` returns 200 with the document's exact source bytes.
- `GET /api/documents/<absent>` returns 404 as a typed `HttpError` — not a thrown exception.
- `GET /api/documents/../../etc/passwd` (and an absolute path) returns 404 and **never reads outside the corpus**.
- `GET /api/errors` reports a corpus with an unreadable file, rather than hiding it.
- **Every** response, 200 and 404 alike, carries `cache-control: no-store, must-revalidate`.
- A request that holds the index across a concurrent `swap` sees one consistent index.
- `./scripts/check-all.sh` green: both gates, npx smoke, tsc, tests, coverage > 90% on all four.
- `gate-vendor-boundary.sh` confirms the router lives under `entrypoints/` and no `node:*`/platform type leaked into `domain/`.

**Verification method** — the commands/tests/probes that prove them:

- Unit tests drive the real `Web` through `handle(app, request)` — no port, no sockets, no flakes — asserting status, body, and headers as data.
- The traversal cases are asserted against a corpus whose fake filesystem would happily serve the file if asked, so a pass means the guard ran and not that the file was merely missing.
- `./scripts/check-all.sh` exit 0.
- A live probe: `insightbrowser serve --port 4100`, then `curl -i` for a 200, a 404, and the `no-store` header.

**Gate** — what must pass before approval:

- `./scripts/check-all.sh` green.
- The traversal guard demonstrated: a path that escapes the corpus 404s and reads nothing.
- The live probe on 4100 answers `/api/documents` and `/api/health`.

## Considerations

- **Tag filtering is out of scope** — it needs ticket 2's front matter. Do not invent a placeholder tag shape here; a stand-in would be a lie the compiler would then help spread (`packages/insightbrowser/src/domain/model/Document.ts`).
- **This is the first test of tonight's index design.** If `IndexRef`/`Index` fits a second consumer badly, that is a finding worth surfacing now, while the design is a day old — not something to route around in the router.
- **Path traversal is the real security surface here.** `asDocumentPath` already rejects `..` segments and absolute paths, and `getDocument` only accepts the brand — but the wildcard capture is raw user input, so the test must prove the guard runs rather than trusting the type (`packages/insightbrowser/src/entrypoints/api.ts`).
- **`serve` is deliberately absent from the CLI today** (ticket 1 declined to stub it). Adding it here means the CLI's `--help` finally describes something real.
- **Do not add an ETag** — `docs/adr/0003` considered and declined it; adding one would reverse a recorded decision.

## Final Report

**Outcome:** Implemented. `./scripts/check-all.sh` exits 0, and the server was driven live against this repository's own corpus.

### What was built

- **`entrypoints/api.ts`** — `GET /api/health`, `/api/documents`, `/api/documents/*path`, `/api/errors`, plus a catch-all. Built on `plgg-server@0.0.3` (`pipe(web(), use(...), get(...))`), written against plgg-http's `HttpRequest`/`HttpResponse` — no platform types, so the Worker/Lambda adapter stays an IO conversion.
- **`entrypoints/serve.ts`** — the composition root: scan `cwd`, hold the index in an `IndexRef`, serve. Structured JSON logs (`scan.complete`, `scan.error`, `serve.listening`), never `console.log` prose.
- **`entrypoints/cli.ts`** — the `serve` verb, `--port` (default 4100, the mission's gate port). `--help` now describes something real.

### Verification actually run

- `./scripts/check-all.sh` → **exit 0**: both gates green, npx smoke passed, tsc clean, **51 tests**, coverage **100% statements / 93.18% branches / 100% functions / 100% lines**.
- **Live, against this repo's own corpus** (`node bin/insightbrowser.mjs serve --port 4100`): scanned **20 documents in 4ms**, `/api/health` → `{"documentCount":20,"errorCount":0}`, and `/api/documents/docs/adr/0003-no-caching.md` returned that ADR's real source bytes. The product read its own reasoning back to me.
- Every endpoint probed for the ADR-0003 header: 200s, 404s, unmatched routes, and traversal attempts **all** carry `cache-control: no-store, must-revalidate`.

### The bug the live probe found (and the unit tests did not)

**404s had no `cache-control` at all.** The first cut set the header on the success path, and the unit test asserted it on a *200* — proving nothing about the error path. A live `curl` of a 404 came back bare. That matters precisely for a 404: cache a "not found" and the document someone just added stays missing, which is exactly the staleness ADR 0003 exists to prevent.

Fixed by making `noStore` a **middleware** that folds the typed `HttpError` into its response at the edge (plgg's own `mapErr`-once-at-the-edge idiom) and stamps the header on whatever comes out. Handlers still return `err(notFound(...))` — they stay honest about failure; only the transport flattens it, which is what HTTP does anyway.

Then a second live finding: **plgg-server answers an *unmatched* path with its own 404 that never passes through global `use()` middleware**, so `/foo` was still bare. Closed with a catch-all route registered last, so every path has a route and every response leaves through the middleware. Both findings are now pinned by tests.

### A claim of mine that was luckier than it looked

The traversal test asserted "the guard ran". Live probing showed the *unencoded* `../../etc/passwd.md` never reaches the handler — HTTP normalizes it into an unmatched path, so the 404 came from dispatch, not from my guard. The **URL-encoded** form (`%2E%2E`) does reach the handler and *is* rejected by `asDocumentPath`, returning 404 with the header. So the guard is real — but the original test would have passed even if it were not. Worth knowing: the encoded case is the one that tests the guard.

### Discovered insights

1. **The anti-corruption claim is now a fact, not a hope.** The CLI and the API start the same `scan`/`Index` procedures identically — the evidence `anti-corruption-structure` asks for. Adding a second surface required **zero** domain changes, which is the strongest signal available that last night's `Index`/`IndexRef` design fits.
2. **`IndexRef`'s central claim survived its first real consumer.** A request that holds the index across a concurrent `swap` sees one consistent value — asserted with a genuinely in-flight request, not a simulation.
3. **`ResponseBody` is a union** (`SoftStr | Bytes | Stream`), so the test helper narrows with `typeof` and fails loudly on a non-text body rather than casting.

### Deviations from the ticket

- **Tag filtering** remains out of scope as planned (blocked on plgg-md 0.0.2).
- **Hot reload is not wired to the server.** `serve` scans once; the domain's `debouncedReload` is built and tested but no `node:fs.watch` calls it yet. Left deliberately absent rather than half-present, so nothing claims to hot-reload until it does. Follow-up ticket needed.
