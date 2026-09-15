#!/usr/bin/env bash
# Stamp the built documentation tree with the commit it carries, immediately before it is
# published. Both publish triggers call this — CI's `docs-build` job for staging, the release
# workflow's `docs-deploy-production` job for production — so the two stamps cannot drift.
#
#   Usage: bash scripts/stamp-docs-deploy.sh <environment>
#     <environment>   staging | production
#
# Run from the repository root, after `npm run docs:build`. Writes into the build output
# (`docs/.vitepress/dist`, gitignored) and never touches a tracked file:
#
#   version.json    every environment — the commit, ref, environment and build time, so
#                   `curl https://<host>/version.json` answers "which commit is this?"
#                   without an authenticated look at Cloudflare.
#   robots.txt      staging only — disallow every crawler. See the caveat below: at the edge
#                   this file is no longer the binding half of the staging guard.
#   _headers        staging only — `X-Robots-Tag: noindex, nofollow` on every path. This is
#                   what actually keeps staging out of an index.
#
# Why `_headers` and not `robots.txt` is the load-bearing one (measured 2026-08-19 against the
# live hostname): Cloudflare PREPENDS a "Managed content" block to whatever robots.txt the origin
# serves, and that block carries its own `User-agent: *` group ending in `Allow: /`. RFC 9309 has
# a crawler merge all groups matching the same user-agent token and, between an allow and a
# disallow of equal specificity, take the least restrictive — so the `Disallow: /` written below
# merges with the injected `Allow: /` and does not bind. It is kept anyway (it is correct at the
# origin, and it binds again the moment the zone's managed robots.txt is turned off), but do not
# read it as the guarantee. Verify staging with the header.
#
# Why staging is public but non-indexable: the repository, `main`, and the released
# documentation are all public already, so restricting the hostname would protect nothing that
# is not already readable. What restriction WOULD have prevented is a search engine ranking
# unreleased documentation above the released pages, and the two lines above prevent exactly
# that without a second credential to hold. Production ships the tracked `docs/public/robots.txt`
# unchanged and is left indexable.

set -euo pipefail

ENVIRONMENT="${1:-}"

case "$ENVIRONMENT" in
  staging | production) ;;
  *)
    echo "usage: bash scripts/stamp-docs-deploy.sh <staging|production>" >&2
    exit 2
    ;;
esac

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$REPO_ROOT/docs/.vitepress/dist"

if [ ! -d "$DIST" ]; then
  echo "no build output at $DIST — run 'npm run docs:build' first" >&2
  exit 1
fi

# GitHub Actions supplies these; a hand-run deploy falls back to the checkout itself.
COMMIT="${GITHUB_SHA:-$(git -C "$REPO_ROOT" rev-parse HEAD)}"
REF="${GITHUB_REF_NAME:-$(git -C "$REPO_ROOT" rev-parse --abbrev-ref HEAD)}"
BUILT_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

cat > "$DIST/version.json" <<JSON
{
  "commit": "$COMMIT",
  "ref": "$REF",
  "environment": "$ENVIRONMENT",
  "built_at": "$BUILT_AT"
}
JSON

if [ "$ENVIRONMENT" = "staging" ]; then
  printf 'User-agent: *\nDisallow: /\n' > "$DIST/robots.txt"
  printf '/*\n  X-Robots-Tag: noindex, nofollow\n' > "$DIST/_headers"
fi

echo "stamped $ENVIRONMENT: commit $COMMIT (ref $REF) built at $BUILT_AT"
