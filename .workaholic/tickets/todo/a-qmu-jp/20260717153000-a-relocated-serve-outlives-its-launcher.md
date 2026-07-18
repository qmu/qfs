---
created_at: 2026-07-17T15:30:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Infrastructure]
effort: 2h
commit_hash:
category: Changed
depends_on:
mission: qfs-viewer-mvp
---

# A relocated `serve` outlives its launcher, and keeps the port

## Overview

Found while extending the npx smoke to the serve path (ticket
`20260717020106`), and left as a finding rather than papered over: the smoke
works around it with `pkill -P`, which is a test's remedy for a product's
problem.

`npx qfs-viewer serve` installed from the registry does not run the server in
the process the caller started. `bin/qfs-viewer.mjs` relocates out of
`node_modules` (Node 24 refuses to strip types from `.ts` there) and
**re-execs the real server as a child**. Signal the launcher and the child
survives — reparented to init, still holding the port.

Measured on this branch (packed tarball, scratch install, node 24.13.1):

```
533296  533187  node ./node_modules/.bin/qfs-viewer serve --port 42627   <- launcher
533304  533296  node /tmp/plgg-relocate-qfs-viewer-0.0.1-…/bin/qfs-viewer.mjs serve --port 42627

$ kill 533296            # the launcher, the only PID the caller has
$ curl /api/health       # {"documentCount":1,"errorCount":0}   <- still serving
533304       1  …/bin/qfs-viewer.mjs serve --port 42627         <- PPID now 1
```

The launcher's own `kill` is the ordinary case: a process manager, a CI step,
a `trap` in a shell script, systemd, or a developer's Ctrl-C in a
non-interactive context all hold exactly that one PID. So the product's
headline command starts a server that its caller cannot stop, and the second
`serve` on the same port then fails to bind against a process nobody knows the
name of.

Not reproducible from a source checkout (`node bin/qfs-viewer.mjs serve`) —
relocation is a no-op there, so the launcher IS the server. It reproduces only
through the packed, installed path, which is exactly the path `npx` takes and
the one nothing but `scripts/smoke-npx.sh` exercises.

## Policies

- `workaholic:operation` / `policies/graceful-degradation.md` — a process a
  supervisor cannot stop cannot be restarted, drained, or rolled; the
  runtime's contract with its manager is part of the product.
- `workaholic:implementation` / `policies/anti-corruption-structure.md` — the
  relocation is a launcher concern (`bin/`), and its lifetime semantics must
  not leak into how the server is operated.

## Implementation notes

Interactive Ctrl-C is NOT the failure: SIGINT goes to the whole foreground
process group, so the child dies too. It is the single-PID signal that leaks,
which is why this hid behind an interactive test.

The likely fix is for the launcher to stop being a parent: `execve`-style
replacement rather than spawn-and-wait, so the relocated server inherits the
launcher's PID and there is only ever one process to signal. If a child is
unavoidable, the launcher must forward SIGTERM/SIGINT to it and reap it — a
handler, not a hope. The relocate bridge is time-boxed debt (docs/adr/0005);
prefer the fix that dies with the bridge.

Note `/tmp/plgg-relocate-qfs-viewer-<version>-<hash>/` is left behind too;
whatever fix lands should say who owns that directory's lifetime.

## Quality Gate

- Acceptance: for the packed, installed bin, `kill <the PID the caller
  started>` stops the server and releases the port, under every runtime the
  smoke covers.
- Verification: `scripts/smoke-npx.sh` drops the `pkill -P` workaround from
  `stop_serve` and still passes — the workaround's removal IS the test.
- Gate: `./scripts/check-all.sh` exits 0.
