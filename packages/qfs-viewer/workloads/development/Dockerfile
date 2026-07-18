# The development workload — `qfs-viewer serve` over the mounted repository.
#
# A *dev* image, not a production artifact: it runs the package's TypeScript
# source directly (Node strips types on load) against the live host tree, which
# is the whole point — the corpus the container serves is the one the developer
# is editing, and hot reload follows it.
#
# Node 24, deliberately: the launcher relies on type stripping being on by
# default, and Node 24's refusal to strip types under `node_modules` is the
# behaviour `bin/relocate.mjs` exists to handle. Pinning an older Node here
# would test a path no developer runs
# (docs/adr/0005-pinned-toolchain-under-min-release-age.md).
FROM node:24-slim

WORKDIR /app

# Clean source only — node_modules/dist are excluded by the repo-root
# .dockerignore. This is a fallback copy: at runtime compose bind-mounts the
# live host repo over /app, so the entrypoint operates on the mounted tree.
COPY . .

EXPOSE 4100

# The bind mount at runtime HIDES anything installed into this image, so the
# install happens in dev-entrypoint.sh AFTER the mount is live.
ENTRYPOINT ["sh", "workloads/development/dev-entrypoint.sh"]
