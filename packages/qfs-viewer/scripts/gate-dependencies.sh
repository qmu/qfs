#!/bin/sh -eu
REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) && cd "$REPO_ROOT"

# The dependency-contract gate.
#
# The mission's central promise is "the plgg family from npm, and nothing
# else". A promise in CLAUDE.md is not checkable; this is. It fails if any
# package declares a RUNTIME dependency that is not plgg-family.
#
# plggmatic is ACCEPTED since ADR 0002's second amendment (2026-07-17): the
# ported engine at packages/plggmatic is this package's UI engine, so the
# exclusion this gate once enforced is gone — an explicit amendment, not a
# silent reversal (docs/adr/0002-plggmatic-is-a-reference-not-a-dependency.md).
#
# Scope: `dependencies` only. devDependencies (typescript, plgg-bundle,
# plgg-test, @types/node) are EXEMPT — the contract governs what ships at
# runtime, and no package could build or test without them. Conflating the two
# would reject a buildable repo.
echo "=== Gate: dependency contract (runtime deps are plgg-family only) ==="

# 1. Prove the gate logic itself is sound (red on a foreign dep, green on a
#    clean package and on plggmatic) every run — a gate never proven to fail
#    is not a gate.
node scripts/dependency-contract.mjs --self-test

# 2. Enforce the contract across every package.
node scripts/dependency-contract.mjs
