# workloads/development

A **development workload**: one container that installs the package and runs
`qfs-viewer serve` over the **mounted** repository — so you can see the
product scan and serve a real corpus (this repository's own) without
installing anything on the host.

This is a dev image (it runs the TypeScript source directly against the live
tree), not a production artifact. The hosted SSR targets on the mission's
roadmap — Cloudflare Worker + D1, Lambda + EFS + sqlite — get their own
workloads when their ticket runs.

## Run it

From the repo root:

```sh
./scripts/serve-development.sh
```

Then open <http://localhost:4100/api/health>.

Compose equivalent (also from the repo root, so the build context is the whole
repository):

```sh
docker compose -f workloads/development/compose.yaml up --build
```

Stop it:

```sh
docker compose -f workloads/development/compose.yaml down
```

## What you should see

```sh
curl localhost:4100/api/health      # {"documentCount":24,"errorCount":0}
curl localhost:4100/api/documents   # every .md in the repo (build output excluded)
curl localhost:4100/api/documents/docs/adr/0003-no-caching.md
```

**The mount is the point.** The container serves the host tree, so:

```sh
echo '# hello' > docs/probe.md      # on the HOST
curl localhost:4100/api/health      # documentCount goes up, within ~50ms
rm docs/probe.md                    # and back down
```

A container serving a frozen copy of the corpus would demonstrate the one
thing this product is not.

## Notes

- **Port 4100** is the mission's `gate_target`, so a bare run lands where the
  acceptance check looks.
- **Node 24** is deliberate: the launcher relies on type stripping, and Node
  24's refusal to strip types under `node_modules` is exactly what
  `bin/relocate.mjs` handles. An older Node here would exercise a path no
  developer runs.
- **`.workaholic` is not `.dockerignore`d** (the plgg monorepo excludes it).
  Here it is *content*, not metadata — the scanner reads it. See the
  `.dockerignore` comment.
- **No `file:` link graph.** Unlike the plgg monorepo's workloads, this is a
  standalone repo consuming the plgg family from npm, so the entrypoint is one
  `npm install` and the compose file needs one `node_modules` volume, not a
  dozen ([ADR 0001](../../docs/adr/0001-npm-only-plgg-family-contract.md)).
- **Pins resolve identically in and out of the container.** Every plgg
  dependency is `^0.0.x`, which npm treats as an exact patch — so the image
  needs no npm config to reproduce the host's versions, even though the host's
  `min-release-age` control does not exist inside it
  ([ADR 0005](../../docs/adr/0005-pinned-toolchain-under-min-release-age.md)).
