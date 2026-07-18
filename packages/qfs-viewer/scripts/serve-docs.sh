#!/bin/sh -eu
REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) && cd "$REPO_ROOT"

# Launch the docs workload (workloads/docs/compose.yaml) DETACHED —
# qfs-viewer serving ANOTHER repository's corpus, READ-ONLY, on
# http://localhost:${WORKAHOLIC_DOCS_PORT:-4101}.
#
#   scripts/serve-docs.sh /home/ec2-user/projects/plgg
#   scripts/serve-docs.sh /home/ec2-user/projects/qfs
#
# Stop it later with:
#
#   DOCS_CORPUS=. <compose> -f workloads/docs/compose.yaml down
#
# The `DOCS_CORPUS=.` is NOT decoration and it is not a wart to tidy away.
# compose.yaml declares the corpus with `:?` (required, no default) so that no
# directory is ever served by accident — and a required variable is required
# for EVERY subcommand, `down` and `logs` included, not just `up`. So the stop
# command needs SOMETHING there; `.` is never mounted because `down` mounts
# nothing. The first draft of this comment omitted it and was a documented
# command that did not work.
#
# WHY: the mission asks for the plgg and qfs documentation sites to be "built
# and served on this mechanism". Neither repository has to adopt anything for
# that to be true — running the tool at a directory is reading it. Measured
# 2026-07-16, unmodified: plgg 954 documents in 186ms, qfs 578 in 191ms.
#
# THE CORPUS IS NOT OURS. Writes are refused twice and independently: the
# server is started `--read-only` (no writer is constructed, so `/edit` does
# not exist), and the mount is `:ro` (the kernel refuses regardless). Do not
# lean on access control here — principals are OPEN when none are declared,
# and a repository that has never heard of this tool declares none.

TARGET="${1:-}"
if [ -z "$TARGET" ]; then
  echo "usage: scripts/serve-docs.sh <path-to-repository>" >&2
  echo "  e.g. scripts/serve-docs.sh /home/ec2-user/projects/plgg" >&2
  exit 2
fi
if [ ! -d "$TARGET" ]; then
  echo "No such directory: $TARGET" >&2
  exit 2
fi

# Resolve to an absolute realpath: compose interprets a relative volume source
# against the compose file's directory, not the caller's, so a relative
# argument would silently mount the wrong tree.
DOCS_CORPUS=$(cd "$TARGET" && pwd -P)
export DOCS_CORPUS
DOCS_CORPUS_NAME=$(basename "$DOCS_CORPUS")
export DOCS_CORPUS_NAME

# Refuse to serve THIS repository. Not a safety rail — a correctness one: this
# workload mounts read-only and would present the developer's own tree as an
# uneditable docs site, which is the development workload's job done wrong.
if [ "$DOCS_CORPUS" = "$REPO_ROOT" ]; then
  echo "That is this repository — use scripts/serve-development.sh (port 4100)," >&2
  echo "which serves it EDITABLE, as the mission intends." >&2
  exit 2
fi

# Credentials come from ONE git-ignored .env at the repo root: its KEY=value
# lines are exported here so a workload's ${VAR:-} compose interpolation picks
# them up with no per-workload wiring. Precedence: a variable ALREADY SET in
# the caller's environment wins. Lines must be plain KEY=value (no
# quoting/expansion); malformed names are skipped with a warning, never eval'd.
#
# This workload needs no credential — it serves public markdown read-only — but
# it reads .env for WORKAHOLIC_DOCS_PORT, and the loading rule belongs in the
# runner rather than being rediscovered per workload (command-scripts policy).
if [ -f "$REPO_ROOT/.env" ]; then
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in '' | \#*) continue ;; esac
    name=${line%%=*}
    value=${line#*=}
    case "$name" in
      '' | *[!A-Za-z0-9_]*)
        echo "warning: skipping malformed .env line (${name})" >&2
        continue
        ;;
    esac
    if eval "[ -z \"\${${name}+x}\" ]"; then
      export "${name}=${value}"
    fi
  done <"$REPO_ROOT/.env"
fi

PORT="${WORKAHOLIC_DOCS_PORT:-4101}"

COMPOSE_FILE="workloads/docs/compose.yaml"
if [ ! -f "$COMPOSE_FILE" ]; then
  echo "No compose file at $COMPOSE_FILE" >&2
  exit 2
fi

# Resolve a real compose engine. A `docker`->`podman` shell alias is
# interactive-only and does NOT exist in this non-interactive script, so we
# must find an actual binary: prefer docker, else podman (this host aliases
# docker to podman).
if command -v docker >/dev/null 2>&1; then
  COMPOSE="docker compose"
elif command -v podman >/dev/null 2>&1; then
  COMPOSE="podman compose"
else
  echo "Need docker or podman to serve the docs workload" >&2
  exit 1
fi

echo "=== Serving ${DOCS_CORPUS_NAME} (read-only) at http://localhost:${PORT} ==="
echo "=== Corpus: ${DOCS_CORPUS} ==="
$COMPOSE -f "$COMPOSE_FILE" up --build -d
echo "=== Ready: http://localhost:${PORT}/api/health ==="
