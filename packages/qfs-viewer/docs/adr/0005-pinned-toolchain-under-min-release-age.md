# 0005 — Pinned toolchain under `min-release-age`, and the relocation bridge

**Status:** Accepted (2026-07-15) — **time-boxed; retire per the schedule below**
**Ticket:** 20260715004234-repository-skeleton-and-dependency-contract.md
**Mission:** build-insightbrowser-on-the-plgg-family

## Context

The development environment sets a supply-chain control in `~/.npmrc`:

```
min-release-age=7
```

npm translates this into `before = now - 7 days`: **any release younger than
seven days is invisible to the installer**. It is a defence against a
compromised package being pulled in during the window before anyone notices —
the standard mitigation for the npm supply-chain attacks of recent years.

The plgg family shipped a burst of releases between 2026-07-09 and 2026-07-13.
At the time of writing (2026-07-15) that means:

| package | latest | published | consumable |
| --- | --- | --- | --- |
| plgg | 0.0.27 | 2026-07-03 | **now** |
| plgg-md | 0.0.2 | 2026-07-09 | 2026-07-16 |
| plgg-view | 0.0.2 | 2026-07-09 | 2026-07-16 |
| plgg-test | 0.0.5 | 2026-07-09 | 2026-07-16 |
| plgg-cms | 0.0.2 | 2026-07-10 | 2026-07-17 |
| plgg-bundle | 0.0.6 | 2026-07-13 | 2026-07-20 |
| plggpress | 0.0.4 | 2026-07-13 | 2026-07-20 |

`plgg-cms` has **no** consumable version at all today — its only release is
five days old.

The installable build tools (`plgg-bundle` 0.0.2, `plgg-test` 0.0.3) predate a
fix that matters: Node 24 refuses to strip types from `.ts` files under
`node_modules`
(`ERR_UNSUPPORTED_NODE_MODULES_TYPE_STRIPPING`). Both tools execute their own
`.ts` source, so **as installed from the registry, they do not run**.

## Decision

1. **Do not weaken the supply-chain control.** `min-release-age` is not
   overridden, per-project or per-command. It is a security control, and
   disabling it to make a build go green inverts the reason it exists.
2. **Pin what is consumable**, and declare only what is imported:
   `plgg ^0.0.27` at runtime; `plgg-bundle ^0.0.2`, `plgg-test ^0.0.3`,
   `typescript ^6.0.3`, `@types/node ^25.6.0` as devDependencies.
3. **Bridge the Node-24 gap with upstream's own remedy.**
   `scripts/plgg-tool.sh` copies a tool out of `node_modules`, points a
   `node_modules` symlink at its deps, and runs it there. This is exactly what
   `plggpress/bin/relocate.mjs` does — we apply it from the outside instead of
   waiting for the fixed bin.
4. **Retire the bridge on a schedule** (below), rather than letting it become
   permanent infrastructure.

## Amendment (2026-07-15 18:xx) — decision 1 was reversed, deliberately

**Decision 1 above no longer describes this repository.** At the developer's
direction, `plgg-md` was bumped to `^0.0.2` ahead of its 2026-07-16 18:11 JST
consumable date, installed with a per-command
`NPM_CONFIG_MIN_RELEASE_AGE=0 ./scripts/npm-install.sh`. Recorded here rather
than left as drift, because decision 1 said this would not be done.

What was and was not weakened:

- `~/.npmrc` is **unchanged** — the floor still applies to every other project
  on the machine and to every plain command here.
- The bump itself lives in the lockfile.
- **`scripts/smoke-npx.sh` now carries a scoped override**, added at the
  developer's direction after the consequence below became visible. It is the
  one committed place the control is off.

### Why the smoke carries an override

Bumping ahead of the date turned `./scripts/check-all.sh` **red**, and only
there. The npx smoke packs the product and installs it as the registry would
serve it — a real consumer, no lockfile — and a consumer respecting the floor
cannot resolve `plgg-md@^0.0.2` until 2026-07-16 18:11 JST:

```
npm error code ETARGET
No matching version found for plgg-md@^0.0.2 with a date before 2026/7/8 18:43:35
```

The override was **declined once and then taken**, and the reasoning moved:

- *Against:* the smoke is the only check that models a real consumer, so an
  overridden smoke reports green while the product is uninstallable.
- *For, and decisive:* that is not the failure the smoke was built to catch.
  Its own header says it exists because "the bin, the `files` list, and the
  launcher could all be broken while every other check stayed green" — Node
  24's `node_modules` type-stripping refusal fails **there and nowhere else**.
  A dependency being too young is a release-readiness fact, not a packaging
  fault. Leaving the gate red on it would have masked the signal the smoke
  actually owns (a launcher regression tomorrow would land on an
  already-red gate and go unseen) and taught the team to ignore a red gate,
  which is a worse and more permanent failure than the one being avoided.

**What it costs, stated so nobody rediscovers it:** this smoke no longer
proves a floor-respecting consumer can install the product. A genuinely
unresolvable dependency now passes it. That check moves to release time — run
the install with no override before publishing or shipping:

```sh
(cd "$(mktemp -d)" && npm init -y >/dev/null && npm install qfs-viewer)
```

The override is **time-boxed**: remove it once plgg-md 0.0.2 clears the floor
(2026-07-16 18:11 JST) unless a newer pin has replaced it. Unlike
`scripts/plgg-tool.sh` — the other time-boxed bridge here, which masks no
failure — this one does mask one. That difference is why it carries a removal
date rather than becoming furniture.

Fixed in passing: the smoke's install swallowed stderr, so the ETARGET failure
exited the gate having printed **nothing**. A check that cannot say why it
failed is barely a check.

**The lesson worth keeping:** `min-release-age` binds *consumers*, not just
this machine. Skipping it did not remove the wait — it moved the failure to
the one check that models a real user, and then to a comment in the script
that silences that check. The 25 hours were never avoidable; they were only
ever relocatable, each time to a place where the fact is easier to forget.

## Reasoning

Three ways out, and why this one:

- **Override `min-release-age`.** Fastest, and wrong. The control exists
  precisely to stop a freshly-published artifact being consumed before the
  ecosystem has had a chance to notice a compromise. Turning it off so that a
  gate goes green tonight trades a real security property for a scheduling
  convenience — and it would be turned off in a config file, where nobody would
  ever notice it again. Declined explicitly.
- **Wait until the versions age in.** Correct but inert: `plgg-bundle` is not
  consumable until 2026-07-20, which would idle the repository's first week
  over a Node-version detail that upstream has already fixed.
- **Apply upstream's own relocation remedy.** Chosen. It invents no new
  technique — `relocate.mjs` exists in the family for exactly this failure, and
  the fixed bins do the same thing internally. We are running the known remedy
  from the outside for as long as the policy hides the bins that carry it.

The bridge is honest about what it is: `scripts/plgg-tool.sh` states the
reason, names the dates, and points here. It does not hide a failure — the gate
it enables is genuinely green (build, typecheck, tests, coverage, and the npx
smoke all really run and really pass).

**This is a dated bridge, not architecture.** The distinction matters: the
danger with a workaround like this is not that it is wrong today, it is that it
outlives its reason and becomes load-bearing. Hence the explicit schedule.

## Retirement schedule

Each step is a small, independent change — do them as the dates pass:

- **~~2026-07-16 18:11 JST~~ → 2026-07-22 21:09 JST** — **remove the
  `NPM_CONFIG_MIN_RELEASE_AGE=0` override from `scripts/smoke-npx.sh`.** Once
  the pinned `plgg-md` clears the floor the smoke resolves it unaided, and the
  check goes back to proving what it is for: that a real, floor-respecting
  consumer can install and run the product. This is the **first** thing to
  retire and the easiest to forget — it is the only committed place the
  control is off, and every day it stays is a day the gate cannot see an
  unresolvable dependency. Verify by running `./scripts/check-all.sh` with the
  override deleted; if it is red, the pins are wrong, which is exactly the news
  the check exists to deliver.

  **The date moved once already, and that is the thing to watch.** It was set
  against `plgg-md 0.0.2` (consumable 2026-07-16 18:11). The upstream YAML-
  subset fix this project asked for shipped as **0.0.3** on 2026-07-15 21:09
  JST, the pin moved to `^0.0.3`, and the seven-day clock restarted:
  `npm install plgg-md@^0.0.3` still answers `ETARGET` today. This is the
  treadmill the override quietly creates — **every upstream fix we ask for and
  adopt pushes the removal date out by another week**, and the heading seam
  (plgg `20260715180322` part 2) will do it a third time. Each extension is
  individually reasonable, which is exactly how a time-boxed workaround becomes
  furniture. If the date has moved twice more with no removal in sight, that is
  the signal to stop extending and either accept the wait or supersede this ADR
  deliberately — not to keep bumping a comment.
- **2026-07-16** — bump `plgg-test` to `^0.0.5`; its bin carries the relocate
  fix. Change `scripts/test-qfs-viewer.sh` back to `npm run test`.
- **2026-07-20** — bump `plgg-bundle` to `^0.0.6`. Change `scripts/build.sh`
  back to `npm run build`. **Delete `scripts/plgg-tool.sh`** — with both tools
  fixed, the bridge has no remaining caller, and a workaround with no caller is
  debt.

Retirement is verified the same way as everything else: `./scripts/check-all.sh`
stays green across each bump.

## Consequences

- `packages/qfs-viewer/package.json` keeps `build`/`test` scripts pointing
  at the tools directly, so the direct path works the moment the pins move —
  the scripts under `scripts/` are what route around the gap.
- Tickets 2 and 3 inherit real dependency-availability constraints:
  - Ticket 2 needs `plgg-md`'s `parseFrontmatter`. Only `plgg-md 0.0.1` is
    consumable today; 0.0.2 lands 2026-07-16.
  - Ticket 3's chosen route (a heading seam added to plgg-md upstream) requires
    a **new** plgg-md release — which this same policy will then hide for seven
    days. That is a genuine, dated blocker on that ticket, not a surprise: it is
    the accepted cost of ADR 0001's registry boundary, and it should be planned
    for rather than discovered.
- Anyone reproducing a green gate on a machine **without** `min-release-age`
  will resolve newer versions than CI does here. That is benign (the pins are
  `^`), but it is why the gate's authority is the local run, per
  `workaholic:operation` / `ci-cd`.
