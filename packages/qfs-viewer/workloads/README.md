# workloads

Execution environment and infrastructure configuration
(`workaholic:implementation` / `directory-structure`) — one directory per
workload, each carrying its own `compose.yaml` (plus a `Dockerfile` and
entrypoint when it builds an image), launched by a `scripts/serve-*.sh`
runner so no invocation lives only in someone's shell history
(`command-scripts`).

| Workload | What it runs | Launch |
| --- | --- | --- |
| [`development/`](development/) | `qfs-viewer serve` over the **mounted** repository, on host port 4100 — the mission's gate port. Edit markdown on the host and the served index hot-reloads. | `./scripts/serve-development.sh` |
| [`docs/`](docs/) | `qfs-viewer serve --read-only` over **another repository's** corpus, on `WORKAHOLIC_DOCS_PORT` (4101). One image, any directory: the corpus is a mount. | `./scripts/serve-docs.sh <path>` |

The two differ in the one line that carries risk, and it is worth stating
plainly: **`development/` grants write authority and `docs/` refuses it.**

That is not a policy setting, it is whose tree it is. "Editable in place" is
the mission's promise for YOUR repository. `docs/` exists to serve repositories
we do **not** own — the mission asks for the plgg and qfs documentation sites
to be "built and served on this mechanism", and neither has to adopt anything
for that to be true, because running the tool at a directory is reading it
(measured 2026-07-16, unmodified: **plgg 954 documents in 186ms, qfs 578 in
191ms**).

Those numbers read 1714 and 1154 an hour earlier, and the difference is worth
keeping: 44% of what this tool reported for plgg was the same repository's
documents seen through `.worktrees/`, at other commits, listed beside
themselves. `.worktrees` is pruned now. A count is only as good as what it
counted, and this one was quoted as evidence before anyone asked.

So `docs/` refuses writes **twice, independently**: `--read-only` means the
writer is never constructed, so `/edit` does not exist rather than existing and
saying no; and the mount is `:ro`, so the kernel refuses whatever the process
believes. Do not lean on access control for this — principals are OPEN when
none are declared, and a repository that has never heard of this tool declares
none.

The hosted SSR targets on the mission's roadmap — Cloudflare Worker + D1, and
Lambda + EFS + sqlite — land here as their own workloads when their ticket
runs (`20260716093913`).

The local developer needs none of this: `npx qfs-viewer` runs against the
working tree directly. The container exists so a contributor can see the
product serve a real corpus without installing a toolchain, and so the same
command works on any machine.
