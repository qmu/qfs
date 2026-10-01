#!/bin/sh -eu
REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) && cd "$REPO_ROOT"

# cluster-console: tsc --noEmit + the plgg-test unit suite (coverage-gated by
# packages/cluster-console/plgg-test.config.json), then prove the console the
# qfs binary embeds (packages/qfs/crates/cluster/ui/index.html) is a fresh
# build of this source.
echo "=== Typechecking packages/cluster-console ==="
( cd "$REPO_ROOT/packages/cluster-console" && npx --no-install tsc --noEmit -p . )

echo "=== Running the packages/cluster-console unit suite (coverage-gated) ==="
./scripts/plgg-tool.sh cluster-console plgg-test src --coverage

./scripts/build-cluster-console.sh --check
echo "\n=== All shell scripts have been executed successfully ==="
