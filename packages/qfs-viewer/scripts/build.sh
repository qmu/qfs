#!/bin/sh -eu
REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) && cd "$REPO_ROOT"

# Build every package dist with the in-house bundler, in dependency order.
# The build TOOL (plgg-bundle) and the whole plgg family are consumed from the
# npm registry as published dependencies, so `npm install` already provisions
# plgg-bundle's own deps (typescript included) — there is no file:-link
# bootstrap step here.
#
# qfs-viewer's plgg-bundle runs via scripts/plgg-tool.sh rather than `npm run
# build`: the pinned 0.0.2 predates the Node-24 node_modules type-stripping
# fix, so it must be run from outside node_modules. See that script's header
# and docs/adr/0005-pinned-toolchain-under-min-release-age.md.
#
# plggmatic (ported from the retired qmu/plggmatic repo, HQ ticket
# 20260716212002) pins plgg-bundle 0.0.6, which carries the relocation remedy
# in its own bin — so `npm run build` works directly there. Neither package
# depends on the other; the order is alphabetical.
echo "=== Building every package dist, in dependency order ==="
( cd packages/plggmatic && npm run build )
./scripts/plgg-tool.sh qfs-viewer plgg-bundle
echo "\n=== All shell scripts have been executed successfully ==="
