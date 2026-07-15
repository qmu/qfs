---
created_at: 2026-07-15T19:00:00+09:00
author: a@qmu.jp
type: housekeeping
layer: [Infrastructure]
effort:
commit_hash:
category:
depends_on:
mission:
---

# Resume qfs development in the new public repository

## Overview

**Read this before running `/ticket`, `/drive`, or any `git log` archaeology in this repository.**
Nothing is half-built — this is not a rescue. It is a handoff written because **the repository changed
identity underneath the work** on 2026-07-15, and a fresh session that assumes the old shape will draw
wrong conclusions confidently.

The publish detour (`20260715151000`, archived) is **complete**: qfs is public and `v0.0.71` is
released.

> **Revised 2026-07-15 (branch `work-20260715-205333`).** Two edits, both because this file was
> itself misleading:
>
> 1. **Absolute dev-box paths removed.** The original body located the two checkouts by absolute
>    path (`/home/<user>/projects/…`). That is one of the four leak classes this very ticket warns
>    about ("dev-box paths into sibling repos"), and the file was still untracked, so it had not
>    published yet. Note `scan-branch-safety.sh` reads only added lines of `git diff base..HEAD` —
>    it never sees an untracked file, so its `pass` was silent about this. Only reading caught it,
>    exactly as the standing obligation below predicts.
> 2. **The "two active missions" section was stale** and is replaced by "Where the work stands"
>    below. See the Changelog at the end.

## The environment changed. This is the part that will bite.

There are **two checkouts** of this project side by side on the development box:

| | |
| --- | --- |
| **this repository** (remote `qmu/qfs`) | the **new public repo**. Fresh, squashed. `git log` shows **3 commits**. This is where work happens now. |
| **the legacy checkout** (a sibling clone, remote `qmu/qfs-legacy`) | the **old private repo**. ~860 commits, all real development history. Read-only reference. Never push from it. |

Identify the legacy checkout by its **remote**, not by a remembered path: it is the clone whose
`origin` is `qmu/qfs-legacy`.

**Consequences a fresh agent will hit, in the order it will hit them:**

1. **`git log` / `git blame` / `git log -S` find nothing here.** Three commits exist and none of them
   are development. Any question of the form *"why is this code like this?"*, *"when did this
   change?"*, or *"what did this look like before?"* must be asked in the legacy checkout via
   `git -C <legacy-checkout>`. The code and `.workaholic/` records are identical in both; only
   history differs.
2. **The `/ticket` discovery step's "Related History" will come up empty** against this repo's git.
   Use `.workaholic/` (tickets, stories, concerns — all carried over intact, ~300 archived tickets)
   as the history surface, and the legacy checkout's `git log` when a commit-level answer is actually
   needed. `.workaholic/` is the better source for *why* regardless; git is only better for *when*.
3. **`check-migrations`' baseline reset to `v0.0.71`.** It passes (verified), and it will correctly
   guard any future in-place edit to a shipped migration body. But it compares against the last
   release tag, and the earliest tag here is `v0.0.71` on the root commit — so "as shipped" now means
   "as of the squash", not "as of the release that shipped each body". Bodies shipped before the
   squash have no earlier baseline in this repo. Not a defect; a consequence to know before trusting
   the ratchet's silence about the pre-squash past.
4. **Do not try to graft the old history onto this repo.** The absence is a decision on file — see
   the root commit's message and the archived publish ticket. It was traded for the guarantee that
   content never present cannot leak.

## Standing obligation, new as of today

**`.workaholic/` publishes.** Every ticket, story, concern, release note, and trip record written from
now on lands in a public tree — including this file. The Gmail / Drive / Slack / Cloudflare accounts
used during development are **still live-connected**, and live-round verification notes are exactly
where real values get pasted verbatim.

So: state what was verified and that it was verified live, but **describe values by shape and
location, never quote them**. This applies to writeups *about* leaks too — the publish ticket's own
first draft quoted every value it documented into a file that publishes, and the git-ignored
`.workaholic/leak-denylist` (a backstop for *known* terms only) is what caught it. Run
`bash <workaholic-plugin>/skills/release-scan/scripts/scan-branch-safety.sh` before committing
records — but see the revision note above: **a `pass` only covers committed, added lines.** Untracked
files and shapeless leaks are invisible to it.

The four leaks found on 2026-07-15 were a client's mail label, a live cloud account's resource
inventory, real document filenames, and dev-box paths into sibling repos. **None had a shape** — no
regex, canary test, or credential scanner could see them. Only reading found them.

## Where the work stands (revised 2026-07-15)

The original version of this section named "two active missions" and called the language mission "the
live one", with a blueprint **type-system chapter** as its next move. **That was wrong and would have
sent the next session to build something that already exists**: the type-system chapter landed
2026-07-09 (ticket `20260709104254`, blueprint §5.1–§5.8). The language mission's acceptance was
11/11 and its own changelog already recorded that its design questions were exhausted.

The missions were reframed on 2026-07-15 (owner-approved). See
[`missions/index.md`](../../../missions/index.md) for the rule; the short version:

- **Missions are standing product properties, not episodes of work.** An activity ends while its
  residue does not — which is how two missions archived `achieved` while their residue was still
  live, leaving every one of the 29 open concerns homed to a finished mission or to nothing.
- **Belonging to no mission is legitimate.** Isolated defects, scope cuts, watch items, cross-repo
  tooling, and the owner-attended live-verification backlog are mission-free and picked up as plain
  tickets.

**Active missions (both design-led, neither has implementation tickets yet):**

1. **`declared-drivers-are-the-normal-way-to-add-a-service`** (0/7) — new; adopts the archived
   capability-tryout mission's unfinished goal #2 plus the roadmap's 🧭 cloud-account-declaration
   gap. Seven concerns re-homed onto it.
2. **`support-create-agent-semantics-…`** (0/6) — `CREATE AGENT` introducing a new principal
   (distinct identity, own least-privilege grant, own audit trail). Owner directive 2026-07-12; the
   building blocks ship already (`t57` policy axes model subjects; the directories driver models
   identities; the v0.0.59 sweeper owns the "when"). Composition work, not new primitives. Its first
   acceptance item is a blueprint agent-model chapter — a design brief, not a code ticket.

## Implementation Steps

This ticket is a **handoff, not a build**. Remaining:

1. ~~Confirm the two-repo split is understood.~~ Done.
2. ~~Ask the owner which mission increment is next.~~ Done — the owner chose to reframe the missions
   and tickets first; that reframing is this branch.
3. **Write implementation tickets against the chosen mission increment** via `/ticket`, using
   `.workaholic/` as the history surface (see consequence 2 above). Per the repo's convention, design
   judgment gets a full prose brief (state / options / trade-offs / recommendation), not a compressed
   multiple-choice — and both active missions are design-led.
4. Archive this ticket once the first real ticket for the chosen increment exists.

## Quality Gate

- The next session states, before proposing work, where development history lives and why `git log`
  here is 3 commits deep.
- No implementation begins without an owner decision on the mission increment; both active missions
  are design-led and neither has a ticket to drive yet.
- Any record written lands the branch-safety scan on `pass`, quotes no value read from a live
  account, **and has been read by a human or agent for shapeless leaks** — the scan alone is not the
  gate.

## Considerations

- **28 open concerns** ([`concerns/index.md`](../../../concerns/index.md), grouped by home since the
  reframing). One is directly relevant to tooling: the branch-safety scanner false-positives on Rust
  `Token::Variant`, which hard-blocks `/ship`; the fix is a cross-repo change in the workaholic
  plugin, deliberately not vendored into qfs.
- Current versions: binary `v0.0.71` (tagged, released with four native tarballs), plugin `0.11.8`.
  The patch-per-shipped-PR rule and the four-plugin-version-field rule both still apply — see
  `CLAUDE.md`.
- The legacy checkout's remote was repointed at `qmu/qfs-legacy` on 2026-07-15 (verified still
  correct 2026-07-15). It had been left pointing at `qmu/qfs`, which — after the rename and the new
  repo taking the old name — resolved to the *new public repo*. A routine push from that checkout
  would have put its entire history into it. **If another checkout or worktree of the predecessor
  ever appears, check its remote before pushing.**

## Changelog

- 2026-07-15 — Created as the post-publish handoff.
- 2026-07-15 — Revised on branch `work-20260715-205333`: absolute dev-box paths removed (they were
  the leak class this ticket documents, and the file was untracked so the scan never saw them); the
  stale "two active missions" section replaced with the post-reframing state; step 2 closed; concern
  count and mission list refreshed.
