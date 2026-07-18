#!/bin/sh -eu
REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) && cd "$REPO_ROOT"

# Run a plgg build tool (plgg-bundle, plgg-test) from OUTSIDE node_modules.
#
# Why this exists
# ---------------
# These tools execute their own `src/**/*.ts` and rely on Node stripping types
# on load. Node 24 REFUSES to strip types for `.ts` under `node_modules`
# (ERR_UNSUPPORTED_NODE_MODULES_TYPE_STRIPPING), so a registry-installed tool
# cannot run in place. Upstream's own remedy is `relocate.mjs` (see
# plggpress/bin/relocate.mjs): copy the package OUT of node_modules, point a
# `node_modules` symlink at its deps, and re-exec there.
#
# The fixed tools carry that remedy in their own bins — but this repository's
# supply-chain policy (`min-release-age=7` in ~/.npmrc) hides releases younger
# than 7 days, so today we can only install plgg-bundle 0.0.2 and plgg-test
# 0.0.3, which predate the fix. Rather than weaken the policy (a security
# control) or pretend the gate is green, this script applies the SAME remedy
# from the outside.
#
# This is a dated bridge, not architecture. Retire it — and pin the fixed
# versions — as each becomes consumable:
#   plgg-test  0.0.5  consumable 2026-07-16
#   plgg-bundle 0.0.6 consumable 2026-07-20
# See docs/adr/0005-pinned-toolchain-under-min-release-age.md.
#
# Usage: scripts/plgg-tool.sh <package> <tool> [args...]

if [ $# -lt 2 ]; then
  echo "usage: scripts/plgg-tool.sh <package> <tool> [args...]" >&2
  exit 2
fi

PACKAGE="$1"
TOOL="$2"
shift 2

PACKAGE_DIR="$REPO_ROOT/packages/$PACKAGE"
TOOL_DIR="$PACKAGE_DIR/node_modules/$TOOL"

if [ ! -d "$TOOL_DIR" ]; then
  echo "$TOOL is not installed in packages/$PACKAGE — run ./scripts/npm-install.sh first" >&2
  exit 1
fi

# Key the relocation by tool + version so a version bump cannot reuse a stale
# copy, and by package so two packages never share one.
VERSION=$(node -p "require('$TOOL_DIR/package.json').version")
DEST="${TMPDIR:-/tmp}/qfs-viewer-relocate-$PACKAGE-$TOOL-$VERSION"
BIN_NAME=$(node -p "
  const b = require('$TOOL_DIR/package.json').bin;
  (typeof b === 'string' ? b : b['$TOOL']).replace(/^\.\//, '')
")

# Rebuild the copy whenever it is absent. The tool's own files are immutable
# for a given version, so a present copy is always current.
if [ ! -f "$DEST/.relocate-ready" ]; then
  rm -rf "$DEST"
  mkdir -p "$DEST"
  cp -R "$TOOL_DIR/." "$DEST/"
  touch "$DEST/.relocate-ready"
fi

# (Re)create the deps link every run: a cached copy may hold a link to a tree
# that has since been reinstalled or removed.
rm -rf "$DEST/node_modules"
ln -s "$PACKAGE_DIR/node_modules" "$DEST/node_modules"

# Run with the PACKAGE as cwd — the tools read the target's tsconfig/config
# from the working directory.
cd "$PACKAGE_DIR"
exec node "$DEST/$BIN_NAME" "$@"
