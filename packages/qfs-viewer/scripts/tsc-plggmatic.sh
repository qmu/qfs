#!/bin/sh -eu
REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) && cd "$REPO_ROOT"

echo "=== Running 'npm run tsc' in packages/plggmatic ==="
cd "$REPO_ROOT/packages/plggmatic" && npm run tsc
echo "\n=== All shell scripts have been executed successfully ==="
