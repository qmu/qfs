---
created_at: 2026-07-16T09:39:13+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure]
effort: 4h
commit_hash:
category: Added
depends_on:
mission: build-insightbrowser-on-the-plgg-family
---

# Hosted SSR on Lambda + EFS + sqlite

## The credential blocker is dead

`20260715225000` carries this item as blocked on "credentials for either half
of the `and/or`", with the evidence `aws sts get-caller-identity` →
`NoCredentials`. That command was run, that output was real, and the conclusion
was still wrong: it asked the **default** profile, and a working profile sat
beside it the whole time.

The developer named it on 2026-07-16 (`profile 'q'`, authorising its use here):

```
$ aws sts get-caller-identity --profile q
{
  "UserId": "AROA4G7MM74K5YQZBNIN4:a+corporate@qmu.jp",
  "Account": "839625015061",
  "Arn": "arn:aws:sts::839625015061:assumed-role/AWSReservedSSO_PowerUserAccess_.../a+corporate@qmu.jp"
}
```

**PowerUserAccess on a live corporate account.** The mission's item is
"Cloudflare Worker + D1 **and/or** Lambda + EFS + sqlite" — so this half alone
satisfies it. Cloudflare remains genuinely absent (`wrangler` not installed, no
account), and that is fine: the `and/or` is why the row had two halves.

This is the **fifth** wall on this branch to fall to one more question, and the
second in a row where the check was real but aimed at the wrong artefact (the
default profile; the npm registry). Read `## Policies` before adding a sixth.

## MEASURED — what is already true (2026-07-16)

**`toFetch` is a genuine seam, and it is Web-standard:**

```ts
// plgg-server/dist/Routing/usecase/toFetch.d.ts
export type Fetch = (request: Request) => Promise<Response>;
export declare const toFetch: (app: Web) => Fetch;
```

So the app is *already* a `(Request) => Promise<Response>`. A Cloudflare Worker
`fetch` handler would be nearly free; **Lambda is the one that needs work**,
because Lambda speaks events, not `Request`. Decide the shim (a Function URL
plus the Lambda Web Adapter, or a hand-written event→`Request` mapper) in the
ADR — do not discover it at deploy time.

**The `FileSystem` seam is SYNCHRONOUS, and this decides the storage:**

```ts
// src/domain/model/Scan.ts
export type FileSystem = Readonly<{
  readDirectory: (dir: SoftStr) => ReadonlyArray<SoftStr>;
  isDirectory: (path: SoftStr) => boolean;
  readFile: (path: SoftStr) => SoftStr;
}>;
```

`vendors/nodeFileSystem.ts` implements it with `readdirSync`/`statSync`/
`readFileSync`. **EFS is POSIX, so the seam works over it UNCHANGED** — which
is the strongest argument for EFS here and worth stating as the reason rather
than reaching for it because the mission said so.

**The corollary matters more than the item:** object storage (R2, S3) is not
POSIX and **cannot** be expressed through a synchronous seam. So the mission's
*other* hosted row — "qmu.app-adaptive: config and documents offloaded to R2" —
is **not** merely behind "the same Cloudflare wall" as `20260715225000` says.
Even handed a Cloudflare account tomorrow, it needs `FileSystem` to become
async, and that reaches `scan`, `reload`, the index, and every caller. It is a
domain change wearing an infrastructure hat. Correct that row when this lands;
do not let someone start it believing an account is all it needs.

## The questions this ticket exists to answer

Answer them in **ADR 0008** (`docs/adr/` holds 0001-0006; ticket
`20260716025007` claims **0007** for the client-JS/WebMCP decision — whichever
lands first takes 0007, so check before writing the file).

1. **Region.** `aws configure get region --profile q` returns nothing. There is
   no default to inherit and no correct answer to guess.
2. **EFS, or bake the corpus into the image?** EFS is the mission's word, and
   the sync seam makes it work — but a Lambda image with the corpus copied in
   needs no VPC, no mount target, no NAT, and costs nothing at rest. EFS earns
   its complexity only if the corpus must change without a redeploy. **Does
   it?** That is the actual question, and it is a product question.
3. **"pre-optimized RAG-indexed document data" — what IS that?** The phrase is
   from the mission and has never been defined. Note the trap: RAG normally
   means embeddings, embeddings normally mean a provider, and
   `printenv OPENAI_API_KEY` is **empty** (re-verified 2026-07-16). So this
   clause may be blocked even though the hosting is not — or it may mean
   something cheaper (the front-matter index, pre-built and shipped) that needs
   no provider at all. **Do not build a RAG pipeline to satisfy a phrase.**
   Get it defined.
4. **The Goal says "no build step, no database".** That is the LOCAL promise —
   `npx qfs-viewer` at a repository root. A hosted deployment with sqlite
   contradicts it on its face. Is hosted an explicit, bounded divergence (most
   likely), or does it mean something is wrong with the shape? The ADR must say
   which, because CLAUDE.md currently tells every reader there is **no
   production target at all**.
5. **Teardown and cost.** This is someone's live corporate account, not a
   worktree. Who deletes the EFS, the VPC, the mount targets? What is the
   monthly floor if nobody does? Write it down BEFORE provisioning: an orphaned
   mount target is a bill that arrives after everyone has stopped thinking
   about this ticket.

## Policies

- `workaholic:planning` / `policies/verify-before-building.md` — this ticket
  exists because a real command answered the wrong question for a day. Every
  claim above carries what produced it. Re-run `aws sts get-caller-identity
  --profile q` before provisioning: an SSO session expires, and "it worked
  yesterday" is exactly the class of belief that put this row in the blocked
  column.
- `workaholic:planning` / `policies/cost-estimation.md` — provisioning EFS +
  Lambda + VPC in account 839625015061 spends real money on a real invoice.
  Question 5 is not paperwork; it is the difference between a deployment and a
  liability.
- `workaholic:implementation` / `policies/directory-structure.md` — universal.
- `workaholic:implementation` / `policies/coding-standards.md` — universal. The
  Lambda event shim receives untrusted input at a boundary: take `unknown`, and
  no `as` to make an SDK's event type fit.
- `workaholic:implementation` / `policies/anti-corruption-structure.md` — the
  Lambda handler is one more entry point over the same app. `toFetch(app)` is
  the whole adapter; if the handler grows logic, it is in the wrong place.
- `workaholic:implementation` / `policies/infrastructure-as-code.md` — whatever
  is provisioned is declared in the repository, not clicked. A hand-made EFS
  nobody can reproduce is worse than none, and teardown is code too.
- `workaholic:implementation` / `policies/objective-documentation.md` — ADR
  0008 records the reasoning for questions 1-5, including the "no database"
  divergence. The alternatives (EFS vs baked image) are real; record why the
  loser lost.
- `workaholic:design` / `policies/modular-monolith-first.md` — SSR + REST + MCP
  + hosted are surfaces of ONE unit. A hosted target does not license a second
  codebase or a fork of the domain.
- `workaholic:operation` / `policies/ci-cd.md` — a deploy path that only exists
  on one laptop is not a deploy path. `.workaholic/deployments/` gets an entry
  (the tunnel contract is the template) and `/ship` reads it.

## Key Files

- `src/entrypoints/serve.ts` — `serveCorpus(cwd, port)`; the Node-specific
  wiring the hosted entry parallels rather than reuses.
- `src/vendors/nodeFileSystem.ts` — the sync POSIX implementation that EFS
  makes work unchanged.
- `src/domain/model/Scan.ts` — the `FileSystem` seam itself; the reason R2 is a
  different problem.
- `.workaholic/deployments/development-tunnel.md` — the contract template, and
  the file that says a merge deploys nothing today.
- `CLAUDE.md` `## Deploy` — states "no production target yet". If this lands,
  it stops being true.

## Quality Gate

### Acceptance Criteria

- ADR 0008 answers questions 1-5, each with the alternative it rejected.
- `.workaholic/deployments/` carries an entry for the hosted target: how to
  deploy, how to verify, **how to tear down**, and what it costs at rest.
- The deployed URL serves the corpus: `/` renders columns, `/api/health`
  answers, `/<path>` renders a document — the same surfaces the local one has.
- The Lambda handler contains no logic beyond `toFetch(app)` plus the event
  shim.
- Infrastructure is declared in-repo and re-appliable from a clean checkout.
- `./scripts/check-all.sh` still exits 0 — the hosted entry must not disturb
  the local one.

### Verification Method

```sh
aws sts get-caller-identity --profile q          # still authenticated
curl -sf https://<deployed>/api/health           # {"documentCount":<n>,...}
curl -sf https://<deployed>/ | grep -q '<h1>'    # SSR, not an error page
./scripts/check-all.sh                           # local surface undisturbed
```

### Gate

- **Nothing is provisioned before ADR 0008 is written and the teardown story
  is in `.workaholic/deployments/`.** This is a live corporate account.
- The mission item is checked ONLY against a URL that answered. The carry
  ticket's own rule, and it is the right one: writing the adapter and never
  running it is a claim, not a gate.
- If question 3 ("RAG-indexed") cannot be defined, **the item ships without it
  and the mission text is corrected** — rather than a pipeline being invented
  to satisfy a phrase nobody can explain.

## Considerations

- **Cloudflare is still absent and that is OK.** `wrangler` is not installed
  and there is no account; the `and/or` means this ticket does not need one.
  Do not install wrangler "to keep options open" — ADR 0005's
  min-release-age story is what a casually-added toolchain costs here.
- **The R2 row is mis-filed.** See above: it needs an async `FileSystem`, not
  just an account. Correcting `20260715225000` is part of this ticket.
- **ADR 0005's first retirement date is 2026-07-22 21:09 JST.** If this work
  runs past it, the `NPM_CONFIG_MIN_RELEASE_AGE=0` override in
  `scripts/smoke-npx.sh` is due a decision rather than another bump.
