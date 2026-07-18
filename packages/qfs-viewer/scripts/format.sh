#!/bin/sh -eu
REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) && cd "$REPO_ROOT"

# Format every package's TypeScript/JSON/Markdown with Prettier. Each package
# carries its own `.prettierrc.json` (printWidth: 50); Prettier discovers the
# nearest one per file, so a single top-level invocation honours them all.
echo "=== Formatting packages/ with Prettier ==="
npx --yes prettier@3 --write "packages/**/*.{ts,json,md}" \
  --ignore-path "$REPO_ROOT/.gitignore"
echo "\n=== All shell scripts have been executed successfully ==="
