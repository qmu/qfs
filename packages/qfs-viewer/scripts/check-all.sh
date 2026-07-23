#!/bin/sh -eu
REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) && cd "$REPO_ROOT"

# The one reproducible gate for the standalone qfs-viewer repo.
#
# Order is deliberate: the two structural gates run FIRST (they are fast and
# they fail on contract violations that make everything downstream
# meaningless), then the dist build, then the runtime smoke, then the unit
# suite. Every package consumes the plgg family from the npm registry, so a
# clean run also proves the published cross-repo contract resolves.
#
# CI calls this script and adds nothing (workaholic:operation / ci-cd:
# inspections are repository scripts run locally; hosted CI is a fresh-clone
# backstop, and a green badge is never the primary health signal).

# The dependency contract: runtime deps are plgg-family only (plggmatic
# included since ADR 0002's 2026-07-17 amendment).
# Self-tests its own red/green logic, then enforces.
./scripts/gate-dependencies.sh

# The vendor boundary: third-party imports confined to vendors/ + entrypoints/.
# Self-tests its own red/green logic, then enforces.
./scripts/gate-vendor-boundary.sh

# Build the dist.
./scripts/build.sh

# The npx smoke: pack, install into a scratch tree, run the bin. The unit
# suites execute TS source (never the packed bin), so a broken launcher or a
# wrong `files` list would otherwise ship green — and `npx qfs-viewer` is
# the product's headline promise.
./scripts/smoke-npx.sh

# qfs-viewer: tsc --noEmit + plgg-test unit suite (coverage-gated).
./scripts/test-qfs-viewer.sh

# plggmatic: tsc --noEmit + plgg-test unit suite (coverage-gated).
./scripts/test-plggmatic.sh

echo "\n=== All shell scripts have been executed successfully ==="
