---
title: qfs-viewer development surface (tunnelled)
environment: development
confirmation_method: api-probe
url: https://insight-browser.qmu.dev
endpoint: http://localhost:4100/api/health
command: ./scripts/serve-development.sh
---

## Procedure

Deploy model: **deploy-from-branch**. The container serves the bind-mounted
working tree, so "deploying" is starting the workload on the branch under
review; the merge deploys nothing.

```sh
./scripts/serve-development.sh
```

This builds `workloads/development/` and starts it detached on host port 4100,
the tunnel's origin. `~/.cloudflared/config.yml` routes
`insight-browser.qmu.dev` → `http://localhost:4100` (ingress rule, before the
terminal `http_status:404` catch-all).

Stop it with:

```sh
<compose> -f workloads/development/compose.yaml down
```

## The docs surface — someone else's corpus, read-only

A **second** route, added 2026-07-16: `plgg-docs.qmu.dev` →
`http://localhost:4101`, served by `workloads/docs/`.

```sh
./scripts/serve-docs.sh /home/ec2-user/projects/plgg
# stop:  DOCS_CORPUS=. <compose> -f workloads/docs/compose.yaml down
```

It exists so the people who own the plgg corpus can SEE their documentation on
this mechanism rather than read a claim about it — the mission's "the plgg and
qfs documentation sites are built and served on this mechanism". Neither
repository adopts anything: running the tool at a directory is reading it.
Measured with `.worktrees` pruned: **plgg 954 documents in ~150ms**, qfs 578.

**The corpus is not ours, so writes are refused twice and independently:**
`serve --read-only` never constructs the writer (so `/edit` 404s — it does not
exist rather than existing and refusing), and the mount is `:ro`, which the
entrypoint PROVES with a write probe before serving. Do not lean on Access or
on principals for this: principals are OPEN when none are declared, and plgg
declares none.

Two things this surface does NOT yet do, stated so nobody assumes otherwise:

- **qfs is not published.** `.env` allots one `WORKAHOLIC_DOCS_PORT` (4101) and
  the workload binds it, so one corpus runs at a time. qfs needs a second port,
  a second instance, and its own hostname.
- **It serves the LIVE working tree, not a clean ref.** plgg's checkout sits on
  `work-20260716-023712`, which is **not pushed** — so this publishes unshared
  work-in-progress to everyone behind Access. That is tolerable for an internal
  review surface and would NOT be for a public docs site. A real site should
  serve `origin/main` from a dedicated checkout.

## Confirmation

Executable, and it must pass before the branch merges:

```sh
curl -sf localhost:4100/api/health          # {"documentCount":<n>,"errorCount":<n>}
curl -sf localhost:4100/api/errors          # what those errors actually are
curl -sf localhost:4100/ | grep -q '<h1>qfs-viewer</h1>'
curl -si localhost:4100/api/health | grep -qi 'cache-control: no-store'
curl -so /dev/null -w '%{http_code}' https://insight-browser.qmu.dev/api/health   # 302
```

What each one proves:

- **`/api/health`** returns a document count > 0 — the scan ran against the
  real tree. It does **not** have to return `errorCount: 0`, and this file used
  to say it did, contradicting CLAUDE.md ("`errorCount` is not expected to be
  0, and a non-zero count is not a regression"). It is currently **1**, so as
  written this gate could never pass — and a gate that can never pass teaches
  everyone to skip the list it sits in. Read `/api/errors` and judge the named
  documents instead of the number.
- **`/`** carries the `<h1>` — the root serves a page rather than the
  `404 Not Found` it once did.
- **`no-store`** is present — [ADR 0003](../../docs/adr/0003-no-caching.md)
  holds on the live surface, not just in tests.
- **A `302` through the tunnel proves ONLY that Access is in front of the
  hostname** — it is *not* evidence the workload is up, and this file used to
  imply it was. Access generates the redirect at the edge, before the request
  reaches the tunnel, so a stopped workload answers 302 exactly like a running
  one. Checked without stopping anything: a path the origin cannot possibly
  serve answers 302 too, with `auth_status: NONE`. The `meta` JWT carrying the
  matched hostname and path is still the cheapest proof the INGRESS RULE fired
  — which is all it ever proved. Whether the workload serves is what the
  `localhost:4100` checks above are for. A **502** does mean the tunnel reached
  for an origin and found none.

## There is no production target yet

This is a **development** surface: one laptop's working tree, published behind
Access for review. It is deliberately not production, and merging this branch
deploys nothing:

- The mission's hosted targets — Cloudflare Worker + D1, and Lambda + EFS +
  sqlite, with R2-offloaded config and no caching — are unbuilt; their tickets
  have not run.
- Nothing is published to npm. `qfs-viewer` is unclaimed; there is no
  release artifact a merge could promote.

So for this branch **the merge to `main` is the release of source only**, and
its confirmation is the pre-merge proof above plus `./scripts/check-all.sh`
exiting 0 on the merged tree. When a hosted target lands, it gets its own
`.workaholic/deployments/<target>.md` with a production `## Confirmation`, and
ship gates on that instead.

## Operational notes for `~/.cloudflared/config.yml`

That file is **shared**: it routes ~34 hostnames for this developer. Two things
learned the hard way while adding this route:

1. **Do not `SIGHUP` cloudflared to reload it.** This build (2026.2.0) **exits**
   on SIGHUP rather than re-reading its config, and the `bash -c` wrapper
   supervising it exits with it — so a HUP intended as a reload takes **every**
   hostname offline. That happened on 2026-07-15. Restart it instead:

   ```sh
   setsid nohup /usr/local/bin/cloudflared tunnel run \
     >> ~/.cloudflared/poc4b-restart.log 2>&1 < /dev/null &
   ```

   **Confirm with `cloudflared tunnel info qmu-dev`, not by grepping the log.**
   The log-grep
   (`grep "Registered tunnel connection" ~/.cloudflared/poc4b-restart.log`) was
   the documented check and it is unreliable: on the 2026-07-16 restart the
   tunnel came up healthy and wrote **nothing** to that file, so the grep
   waited forever on a working tunnel. It also counts historical lines, so a
   log with yesterday's four registrations "passes" for a tunnel that is down
   right now.

   `tunnel info` asks Cloudflare instead of asking a file we hope was written:

   ```sh
   cloudflared tunnel info qmu-dev
   # CONNECTOR ID  CREATED               ...  EDGE
   # e6dc9356-...  2026-07-16T02:31:59Z  ...  1xnrt01, 1xnrt05, 2xnrt14
   ```

   The `EDGE` column summing to **four** is a healthy tunnel, and `CREATED`
   proves it is THIS run rather than a stale one. Same lesson as the 302 above:
   ask the thing itself.

   Do not trust `pgrep -f "cloudflared tunnel run"` either: it matches its own
   command line and will report a dead tunnel as alive. `ps -eo pid,lstart,cmd`
   with a `[c]loudflared` bracket-grep shows the real start time.

   Expect the stop to be SLOW — a `TERM` took ~3.5 minutes to drain on
   2026-07-16. Wait for the process to actually exit before starting the new
   one, or you get two.

2. **Validate before touching the running tunnel**, and back the file up first
   (the directory's `config.yml.bak-*` files are the convention):

   ```sh
   cloudflared tunnel ingress validate
   cloudflared tunnel ingress rule https://insight-browser.qmu.dev
   ```

   The rule must sit **before** the terminal `- service: http_status:404`;
   ingress rules match in order, so anything after the catch-all is dead.

DNS needs no work: `*.qmu.dev` already resolves to the tunnel.
