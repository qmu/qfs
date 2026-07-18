#!/bin/sh -eu
REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) && cd "$REPO_ROOT"

# tsc --noEmit + the plgg-test unit suite (coverage-gated by
# packages/plggmatic/plgg-test.config.json).
#
# Unlike qfs-viewer, plggmatic pins plgg-test 0.0.5, which carries the
# Node-24 relocation remedy in its own bin — so it runs straight from
# node_modules and needs no scripts/plgg-tool.sh detour.
echo "=== Typechecking packages/plggmatic ==="
./scripts/tsc-plggmatic.sh

echo "=== Running the packages/plggmatic unit suite (coverage-gated) ==="
cd "$REPO_ROOT/packages/plggmatic" && npx --no-install plgg-test src --coverage
echo "\n=== All shell scripts have been executed successfully ==="
