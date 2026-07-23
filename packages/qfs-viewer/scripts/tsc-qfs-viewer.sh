#!/bin/sh -eu
REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) && cd "$REPO_ROOT"

echo "=== Running 'npm run tsc' in packages/qfs-viewer ==="
cd "$REPO_ROOT/packages/qfs-viewer" && npm run tsc
echo "\n=== All shell scripts have been executed successfully ==="
