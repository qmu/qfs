#!/bin/sh -eu
REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) && cd "$REPO_ROOT"

# The vendor-boundary gate, ported from the plgg monorepo (ticket
# 20260704185201). Enforces the vendor-isolation policy as a machine-checked
# rule: a third-party import (`node:*`, the tsc API, any bare non-plgg
# specifier) may appear in PRODUCTION code ONLY under a package's
# `src/vendors/` or `src/entrypoints/` — the anti-corruption boundary and the
# thin program checkpoints. plgg-family packages are domain vocabulary,
# importable anywhere (see plgg/.workaholic/constraints/architecture.md).
#
# A package with a violation must be listed in
# scripts/vendor-boundary-exemptions.txt; an exempted-but-clean package is a
# STALE exemption. `qfs-viewer` passes unexempted.
#
# One source of truth, reused by scripts/check-all.sh AND the run-tests CI
# workflow (which invokes check-all.sh), so a change that leaks a vendor import
# into the domain fails both locally and in CI.
#
# The analyzer uses the already-present `typescript` package — zero new
# dependencies.
echo "=== Gate: vendor boundary (third-party imports confined to vendors/ + entrypoints/) ==="

# 1. Prove the gate logic itself is sound (red on a violation / stale
#    exemption, green on a clean tree) every run.
node scripts/vendor-boundary-analyzer.mjs --self-test

# 2. Enforce the boundary across every package against the exemption list.
node scripts/vendor-boundary-analyzer.mjs
