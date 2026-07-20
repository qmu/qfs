#!/bin/sh -eu
REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) && cd "$REPO_ROOT"

# Install every package's dependencies. Unlike the plgg monorepo, this is a
# standalone repo: every package consumes the plgg family from the npm
# registry as published `^version` deps, so there is no intra-repo `file:`
# link and no dependency ordering to respect — each package installs on its
# own. (Add packages to this list as they land; the order is alphabetical
# until a real dependency forces otherwise.)
echo "=== Running 'npm install' in every package ==="
cd "$REPO_ROOT/packages/plggmatic" && npm install
cd "$REPO_ROOT/packages/qfs-viewer" && npm install
echo "\n=== All shell scripts have been executed successfully ==="
