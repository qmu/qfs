# 0009 — qfs is FOUND, not bundled and not fetched

**Status:** Accepted (2026-07-17)
**Ticket:** 20260717020106-keep-npx-distribution-true-through-the-ui-replacement.md
**Mission:** qfs-viewer-mvp

> Numbered 0009, not 0008: ticket 020105 claimed 0008
> (`0008-corpus-from-the-qfs-collection-path.md`) on a branch that had not
> merged when this was written. Two unmerged branches cannot both hold a
> number, and the later writer yields.

## Decision

`npx qfs-viewer` acquires its qfs by **finding one already on the machine**.

- The default connection spawns the binary **named `qfs`**, resolved through
  `PATH` (`domain/model/Connection.ts`, `defaultConnection`).
- A qfs living elsewhere is **named, not discovered**:
  `{"qfs": {"form": "spawn", "bin": "/path/to/qfs"}}` in
  `qfs-viewer.config.json`.
- The package **ships no binary** and **downloads none** — no
  per-platform `optionalDependencies`, no `postinstall`, no fetch at first
  run. There is no code path in this package that can put a qfs on a disk.
- When no qfs is reachable, the viewer **still starts**, and says — once at
  boot (`qfs.unreachable`) and again on any query that needed it — what is
  missing, how to get it, and what works meanwhile
  (`unreachableAdvice`, one source for both).

So the acquisition story is exactly one sentence long: **the binary is
named, and the user supplies it.** This ADR exists because that sentence had
two real alternatives, both with working precedent, and neither was chosen
for a reason readable from the code.

## Reasoning

**The connections are the user's, and they live in their qfs.** This is the
decisive one, and it is what makes qfs unlike the binaries this pattern was
invented for. esbuild's executable is stateless: any copy of the right
version is the same copy, which is exactly why shipping one per platform is
safe there. qfs is the opposite. It holds an operator identity and
envelope-encrypted credentials in a SQLite project DB, unlocked by a vault
whose KeyGuardian slots `qfs init` enrolled — and that store is **per
machine, not per binary**: `qfs init` readies "the machine (once)" (qfs
`docs/guide/connect.md`), landing in the OS user's own config directory
(`~/.config/qfs` here). What the viewer is asked to browse is `/mail`,
`/drive`, `/sql/...`: paths that mean nothing except against the store the
person already initialized, enrolled and connected.

So a qfs we shipped would **not** be a sealed second world with an empty
vault — that would almost be safer. It would be a *second binary*, at *our*
pinned version, reaching into the *same* user-owned encrypted state as the
qfs they installed: version skew against a credential store, which is the
worst possible place to discover that two versions disagree about a
migration. And "which qfs am I running?" would stop having an answer the
user could give. Finding theirs on `PATH` means the qfs that answers is the
qfs they authorized, at the version they installed, holding the connections
they made. Nothing else we could do is as correct, and it costs us no code.

**The issuance form must stay the user's choice** (`workaholic:design` /
`user-sovereignty`, and the plan's three forms). Shipping a binary inside
the npm package hard-wires the local, on-demand form into the artifact: the
copy is there whether or not it is wanted, and a deployment that means to
dial a remote qfs (form ③) would be carrying — and silently preferring — a
local one it must not use. `bin` as a config value keeps all three forms
peers, which is the seam ticket 020101 built and this ticket must not undo.

**qfs is a data-plane dependency across a process boundary, not a library.**
The mission says so explicitly, and the sentence has teeth here: bundling is
how a process boundary quietly becomes a coupling. We would own qfs's
release cadence in our version range, and our `^` bump would move someone's
substrate under them. Spawning a named binary keeps the boundary a boundary — the
version is a fact about the machine, which is why `qfs.ready` logs the
version it found (`qfs 0.0.71` here) rather than one we could have printed
from a constant.

**And the dependency contract already forbids the mechanism.** The
esbuild-style route needs `optionalDependencies` on `@qfs/linux-x64`-shaped
packages; `scripts/dependency-contract.mjs` classifies every runtime
dependency not named `plgg*` as `foreign` and fails the gate (ADR 0001).
That is not the reason — it is the reason written down twice, which is what
a healthy gate looks like: the contract was derived from the same
boundary-keeping instinct, so it bites here before a human has to.

## Alternatives considered

- **esbuild-style per-platform npm binary packages** (`optionalDependencies`
  + `os`/`cpu`, the pattern the ticket names). Genuinely the best DX on
  offer: `npx qfs-viewer` would work on a machine that had never heard of
  qfs, with no network step of our own and npm doing the platform matching.
  Rejected because it answers the wrong question — the blocker for a new
  user is not *having a qfs binary*, it is *having a qfs with their services
  connected*, which no bundle can ship. What it would add is a second binary
  at our version over their machine's one store (see Reasoning), plus
  publish infrastructure per platform in a repo that builds no binaries, and
  it trips the dependency gate.
- **`postinstall` download** (fetch the release tarball at install time, the
  way qfs's own `install.sh` does). Rejected: `npx` runs postinstall, so the
  headline command would silently pull an executable from the network on
  first use — and be a no-op under `--ignore-scripts`, which is exactly the
  posture a security-conscious install takes, leaving the careful user with
  the broken build. It also contradicts this repo's own stated threat model:
  ADR 0005 makes us wait seven days on *npm* releases because unreviewed
  code arriving at install time is the risk worth paying for. Fetching a
  tarball from GitHub Releases in a postinstall is that same risk with none
  of the registry's mitigations. The identical script run by the user is the
  same bytes with the sha256 check and — the whole difference — their
  consent.
- **Search likely paths** (`~/.local/bin/qfs`, `/usr/local/bin/qfs`, …) when
  `PATH` misses. Rejected: `PATH` is the mechanism for "where this user's
  binaries are", and second-guessing it is how a viewer ends up running a
  qfs its user did not mean to run. If it is not on `PATH` and not in the
  config, we do not get to guess.
- **Refuse to start without qfs.** Rejected, and it is worth naming: qfs is
  the *mandatory substrate* per the mission, so exiting non-zero has a real
  argument behind it. But demo leg 2 — browse `docs/` markdown as columns —
  needs no qfs at all, and the corpus is scanned in-process. Withholding the
  product's working half over the unavailable half serves nobody. Non-fatal
  probe, loud line, serve regardless.

## Consequences

- **The one thing we owe a user without qfs is the exact command**, and it
  lives in the domain as `QFS_INSTALL_COMMAND` — kept verbatim in step with
  qfs's own README (`curl -fsSL …/packages/qfs/install.sh | sh`, sha256-
  verified, `~/.local/bin`). A wrong command is worse than none: it sends
  someone to a 404 with our name on it. If qfs's README moves, this constant
  is the thing to move with it.
- **A missing qfs is a supported state, not a fault.** `serve` boots, logs
  `qfs.unreachable`, markdown browsing works, and only qfs paths fail — each
  with the same words. `scripts/smoke-npx.sh` asserts precisely this against
  the packed tarball, so the promise cannot rot silently.
- **The smoke can prove the no-qfs path on any machine** (it points `bin` at
  a path that does not exist) but proves the with-qfs path on none — a
  machine without qfs is a legitimate CI host. The happy path is the
  mission's final-demo leg (`020107`), where a human watches a real qfs
  answer.
- **Version skew moves to the user, visibly.** A qfs too old for a query
  fails as qfs's own structured error, not ours. `qfs.ready`'s version line
  is the only record of which qfs a session was talking to — the first thing
  to read when a query behaves unlike the docs.
- **This is reversible, and cheaply.** Nothing above is load-bearing on the
  absence of a binary: the connection is already a closed union, acquisition
  already sits behind `probeQfs` + `defaultConnection`, and an
  `optionalDependencies` scheme would add a form beside the three rather
  than rewrite them. If the demo audience turns out to be people with no
  qfs, this is revisited — with the shared-credential-store problem still to
  answer.
- If qfs ever ships a plgg-family npm package that *is* the binary
  distribution, the dependency-contract objection evaporates and the
  user-store objection does not. This ADR would need amending on the
  sovereignty argument, not the packaging one.
