#!/bin/sh -eu
# workloads/docs/docs-entrypoint.sh — the docs container's runtime command. It
# runs AFTER compose.yaml bind-mounts /app (the tool) and /corpus (someone
# else's repository, read-only).

# The install lands on the MOUNTED tool tree, where Node actually resolves it.
# One package, one install: this repo consumes the plgg family from the npm
# registry (docs/adr/0001-npm-only-plgg-family-contract.md).
cd /app
echo "=== npm install packages/qfs-viewer ==="
(cd packages/qfs-viewer && npm install)

# Refuse to serve nothing. An empty or absent mount would otherwise start
# happily and serve a corpus of zero documents, which looks like a broken
# product rather than a missing argument — and the person reading the page
# would have no way to tell those apart.
if [ ! -d /corpus ]; then
  echo "FAIL: /corpus is not mounted — set DOCS_CORPUS to the repository to serve" >&2
  exit 2
fi

# Prove the mount is read-only rather than trusting it. `--read-only` already
# means no writer is constructed, so this asserts the SECOND, independent
# guarantee — the one that holds even if the first is ever regressed by a
# refactor. Two guarantees that are never checked are one guarantee.
if touch /corpus/.qfs-viewer-write-probe 2>/dev/null; then
  rm -f /corpus/.qfs-viewer-write-probe 2>/dev/null || true
  echo "FAIL: /corpus is WRITABLE — it must be mounted :ro" >&2
  echo "      This workload serves repositories we do not own." >&2
  exit 2
fi
echo "=== /corpus is read-only (verified, not assumed) ==="

# `serve` scans process.cwd(), so the working directory IS the corpus
# selection. Running from /corpus is what makes one image serve any repository.
#
# `--read-only` is not optional here and not a default to be tidied away: this
# tree belongs to someone else. See workloads/docs/compose.yaml for why writes
# are refused twice.
cd /corpus
echo "=== qfs-viewer serve --read-only on :4101 (${DOCS_CORPUS_NAME:-corpus}) ==="
exec node /app/packages/qfs-viewer/bin/qfs-viewer.mjs \
  serve --port 4101 --read-only
