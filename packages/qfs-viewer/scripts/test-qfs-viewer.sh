#!/bin/sh -eu
REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) && cd "$REPO_ROOT"

# tsc --noEmit + the plgg-test unit suite (coverage-gated by
# packages/qfs-viewer/plgg-test.config.json).
#
# The two run separately rather than through the package's `test` script
# because plgg-test must be launched from outside node_modules on Node 24 —
# see scripts/plgg-tool.sh and
# docs/adr/0005-pinned-toolchain-under-min-release-age.md. `npm run test`
# stays in package.json as the direct path for when the fixed plgg-test is
# consumable.
echo "=== Typechecking packages/qfs-viewer ==="
./scripts/tsc-qfs-viewer.sh

echo "=== Running the packages/qfs-viewer unit suite (coverage-gated) ==="
./scripts/plgg-tool.sh qfs-viewer plgg-test src --coverage
echo "\n=== All shell scripts have been executed successfully ==="
