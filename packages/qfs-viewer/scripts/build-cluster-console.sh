#!/bin/sh -eu
REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) && cd "$REPO_ROOT"

# Build the qfs cluster console (packages/cluster-console) and copy it into
# the Rust crate that embeds it. The console is a plggmatic app: plgg-bundle's
# "app" target inlines the plgg family into ONE es module, and this script
# wraps that module in ONE self-contained index.html (no external asset). The
# html is COMMITTED under packages/qfs/crates/cluster/ui/ so `cargo build`
# never needs node; re-run this script after changing the console and commit
# the regenerated file.
#
#   ./scripts/build-cluster-console.sh           # build + copy
#   ./scripts/build-cluster-console.sh --check   # build, fail if the copy is stale
PKG="$REPO_ROOT/packages/cluster-console"
TARGET="$REPO_ROOT/../qfs/crates/cluster/ui/index.html"

echo "=== Building packages/cluster-console ==="
./scripts/plgg-tool.sh cluster-console plgg-bundle
node "$PKG/scripts/wrap-html.mjs" "$PKG/dist/main.es.js" "$PKG/dist/index.html"

if [ "${1:-}" = "--check" ]; then
  if ! cmp -s "$PKG/dist/index.html" "$TARGET"; then
    echo "stale: $TARGET differs from a fresh build; run ./scripts/build-cluster-console.sh" >&2
    exit 1
  fi
  echo "cluster console embed is up to date"
else
  cp "$PKG/dist/index.html" "$TARGET"
  echo "wrote $TARGET"
fi
echo "\n=== All shell scripts have been executed successfully ==="
