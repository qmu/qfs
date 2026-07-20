THE MOST IMPORTANT RULE - `as` `any`, and `ts-ignore` is STRICTLY PROHIBITED as a solution to type errors under any circumstances.

* This repository works under the workaholic engineering standards — refer to the
  `workaholic:workaholify` gateway skill for the policies and the
  working-directory ground rules. The rules live in the policy skills, not here.
* This is the standalone home of **qfs-viewer**: a markdown knowledge browser
  served as SSR HTML + REST API + MCP, run as `npx qfs-viewer` at a
  repository root. It consumes the plgg family — `plgg`, `plgg-view`, `plgg-md`,
  `plggpress`, `plgg-cms` — from the npm registry as published `^version`
  dependencies, NOT from a sibling checkout. No non-plgg dependencies.
* **plggmatic lives here as a sibling package, and is the UI engine.** The
  plggmatic UI engine was ported into `packages/plggmatic` when the
  `qmu/plggmatic` repository was retired (2026-07-16, HQ ticket 212002; git
  history stays in the retired repo). Per ADR 0002's second amendment
  (2026-07-17), `packages/qfs-viewer` consumes it **from the npm registry**
  (`plggmatic: ^0.2.0`, published from this repo the same day) exactly like
  the rest of the plgg family — NOT via a `file:` link, which would break
  the npx smoke. The trail's columns render as the engine's strip
  (`entrypoints/columns.ts`), and the trail lowers into the engine's Scene
  (`domain/usecase/scene.ts`). See
  `docs/adr/0002-plggmatic-is-a-reference-not-a-dependency.md` for the whole
  decision history.
* House coding style (type-driven design, Option/Result, exhaustive `match`, the
  no-escape-hatch rule) matches the plgg monorepo — follow it when writing any
  `packages/` TypeScript.
* Format with Prettier; every package carries its own `.prettierrc.json`
  (`printWidth: 50`) — don't hand-pack onto fewer lines.

## Deploy

There is **no production target yet**, so a merge deploys nothing — the release
is of source only. The one real target is the development surface:

```sh
./scripts/serve-development.sh   # workloads/development/ -> http://localhost:4100
```

It is published at `insight-browser.qmu.dev` through the shared `qmu-dev`
cloudflared tunnel. The full contract, including the operational rules for that
shared tunnel, is `.workaholic/deployments/development-tunnel.md` — the source
`/ship` reads. The mission's hosted shapes (Cloudflare Worker + D1, Lambda +
EFS + sqlite) get their own entries when their tickets run.

## Verify

```sh
./scripts/check-all.sh                  # the one reproducible gate; must exit 0
curl -sf localhost:4100/api/health      # {"documentCount":<n>,"errorCount":<n>}
curl -sf localhost:4100/api/errors      # what those errors actually are
```

**`errorCount` is not expected to be 0, and a non-zero count is not a
regression.** Run against this repository it is currently `{"documentCount":33,
"errorCount":1}` — the count tracks the markdown in the tree, so it moves every
time a ticket or story is written and is not a number to match exactly. A
rejected document is still indexed and served — only its front matter is `None`
— and `/api/errors` names each one with its line. Read `/api/errors` before
treating a count as a fault.

The one current rejection is `.workaholic/stories/work-20260715-003158.md`: the
`/report` story format writes a **multi-line** flow sequence (`tickets:` then
`[` on its own line), and plgg-md's subset takes flow sequences only on one
line. Single-line `layer: [Domain, Infrastructure]` parses fine since plgg-md
**0.0.3**, which fixed the gap this project filed upstream
(plgg `20260715180322`) — before it, 7 of 28 here and 472 of plgg's 661 were
rejected, i.e. every workaholic ticket.

What stays rejected is meant to: `&` aliases, `!!` tags, merge keys, and
`|`/`>` block scalars are genuine attack surface, and the subset is
fail-closed on purpose.

`check-all.sh` runs both gates, the dist build, the npx smoke, typecheck, and
the coverage-gated suite.

**A 302 from `https://insight-browser.qmu.dev` does NOT mean the workload is
up.** This file said it did, and that was wrong in the one direction that
costs you something: it reads a dead workload as healthy. The 302 is
Cloudflare Access's login redirect, generated at the edge *before* the request
reaches the tunnel — so it is returned whether the workload is running or
stopped. Checked without stopping anything: a path the origin cannot possibly
serve answers 302 too, pointing at
`qmu-dev.cloudflareaccess.com/cdn-cgi/access/login/…` with `auth_status:
NONE`. Nothing behind Access influenced that answer, because Access never
asked.

So the 302 proves exactly one thing: Access is in front of the hostname. To
learn whether the workload is actually serving, ask the origin directly — that
is what the `localhost:4100` checks above are for. A **502** does mean the
tunnel reached for an origin and found none.
