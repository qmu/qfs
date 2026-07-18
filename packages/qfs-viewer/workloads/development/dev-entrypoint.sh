#!/bin/sh -eu
# workloads/development/dev-entrypoint.sh — the dev container's runtime
# command. It runs AFTER compose.yaml bind-mounts the host repo over /app, so
# the install below lands on the mounted tree where Node actually resolves it.
cd /app

# One package, one install. Unlike the plgg monorepo's workloads there is no
# dependency-ordered `file:` link graph to walk: this repo consumes the plgg
# family from the npm registry
# (docs/adr/0001-npm-only-plgg-family-contract.md).
#
# The pins resolve identically inside and outside the container. Every plgg
# dependency is a `^0.0.x` caret, which npm treats as an EXACT patch — so the
# container gets plgg 0.0.27 and plgg-server 0.0.3 whether or not the host's
# `min-release-age` supply-chain control is present here (it is not; it lives
# in the developer's ~/.npmrc). That is why the image needs no npm config of
# its own to stay reproducible.
echo "=== npm install packages/qfs-viewer ==="
(cd packages/qfs-viewer && npm install)

# Serve the MOUNTED repo. `serve` scans process.cwd(), so the working directory
# is what decides the corpus: /app is the host tree, which is what makes the
# hot reload visible from the developer's editor.
#
# The bin (not `npm start`) because it is the same entry `npx qfs-viewer`
# runs — a container that exercised a different path would prove less.
echo "=== qfs-viewer serve on :4100 ==="
exec node packages/qfs-viewer/bin/qfs-viewer.mjs serve --port 4100
