# The docs workload — `qfs-viewer serve --read-only` over a MOUNTED
# foreign corpus.
#
# Identical to the development image in every way that matters, and it stays a
# separate file rather than a shared one because the two differ in the only
# line that carries risk: this entrypoint passes `--read-only` and serves
# `/corpus`, and that difference should be readable in the file you are looking
# at rather than hidden behind a build arg.
#
# Node 24, for the same reason development pins it: the launcher relies on type
# stripping being on by default, and Node 24's refusal to strip types under
# `node_modules` is the behaviour `bin/relocate.mjs` exists to handle
# (docs/adr/0005-pinned-toolchain-under-min-release-age.md).
FROM node:24-slim

WORKDIR /app

# Clean source only — node_modules/dist are excluded by the repo-root
# .dockerignore. A fallback copy: at runtime compose bind-mounts the live host
# repo over /app, and the corpus separately over /corpus.
COPY . .

EXPOSE 4101

# The bind mount at runtime HIDES anything installed into this image, so the
# install happens in the entrypoint AFTER the mount is live.
ENTRYPOINT ["sh", "workloads/docs/docs-entrypoint.sh"]
