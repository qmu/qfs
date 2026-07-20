#!/bin/sh -eu
REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) && cd "$REPO_ROOT"

# Launch the development workload (workloads/development/compose.yaml) DETACHED
# — qfs-viewer serving THIS repository's own corpus on
# http://localhost:4100, the mission's gate port.
#
#   scripts/serve-development.sh
#
# The container bind-mounts the repo, so editing any markdown on the host
# hot-reloads the served index. Stop it later with:
#
#   <compose> -f workloads/development/compose.yaml down
#
# Credentials come from ONE git-ignored .env at the repo root: its KEY=value
# lines are exported here so a workload's ${VAR:-} compose interpolation picks
# them up with no per-workload wiring. Precedence: a variable ALREADY SET in
# the caller's environment wins. Lines must be plain KEY=value (no
# quoting/expansion); malformed names are skipped with a warning, never eval'd.
#
# Nothing needs a credential yet — the corpus is public markdown. This is here
# because the voice (OPENAI_API_KEY) and RBAC surfaces on the mission's
# roadmap will, and the loading rule belongs in the runner rather than being
# rediscovered per workload (command-scripts policy).
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

COMPOSE_FILE="workloads/development/compose.yaml"
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
  echo "Need docker or podman to serve the development workload" >&2
  exit 1
fi

echo "=== Serving qfs-viewer at http://localhost:4100 (detached) ==="
$COMPOSE -f "$COMPOSE_FILE" up --build -d
echo "=== Ready: http://localhost:4100/api/health ==="
