---
created_at: 2026-07-15T15:10:00+09:00
author: a@qmu.jp
type: housekeeping
layer: [Infrastructure]
effort:
commit_hash:
category: Changed
depends_on:
mission:
---

# Publish as a fresh repository with squashed history

## Overview

This repository is private and is going public. **Publishing exposes the whole history, not just `main`** — 881 commits across 41 branches all become readable, and `git log -S<term>` finds anything any of them ever contained. Two things must not ship, and only one of them is visible on `main` today:

| # | where | what | visible on `main`? |
| --- | --- | --- | --- |
| 1 | `.workaholic/tickets/archive/work-20260711-121525/20260711010500-docs-slack-user-token-posting-guide.md:18` | an aside naming **another organisation's chat workspace** as where the guide was verified | yes |
| 2 | commits `be5cfbd`, `fdc7791` | a list of **another organisation's cloud resource names** (KV namespaces, worker names) pasted verbatim into a driver checkpoint | **no — history only** |

Item 2 is the dangerous one precisely because it is invisible: reviewing `main` before publishing would find nothing, and the names surface the moment the repo goes public.

**Decision (developer, 2026-07-15): publish as a fresh repository with a squashed history**, rather than rewriting 881 commits across 41 branches. Rewriting would leave the pre-rewrite commits retrievable by SHA until GitHub garbage-collects them — a request only Support can service — whereas a fresh repository never contains them at all. The development history is deliberately traded away for that guarantee.

**A squash preserves the tree.** It removes item 2 (which lives only in old commits) and does nothing about item 1 (which lives in the current tree). Item 1 must be fixed *before* the squash, or it ships.

## Policies

- `workaholic:safety` / `policies/standard.md` — the prohibition on naming other projects, their systems, or their resources in artifacts shared beyond their permitted scope. Public is the widest scope, and this is a one-way door.
- `workaholic:design` / `policies/defense-in-depth.md` — prefer a shape where the unwanted content cannot be present (a fresh repo) over one that relies on removal having been thorough (a rewrite).
- `workaholic:implementation` / `policies/objective-documentation.md` — record what was traded away (the history) and why, so the absence is a decision on file rather than an accident someone later "fixes".

## Implementation Steps

1. **Fix the tree first.** Rewrite the line at `…-docs-slack-user-token-posting-guide.md:18` so it states where the guide was verified without naming the other organisation's workspace — "verified in production against a real Slack workspace on 2026-07-11" carries the same weight. The claim is that it was verified live; whose workspace it was is not load-bearing.
2. **Sweep the tree for anything else of that shape** before squashing, because after the squash the tree *is* the entire repository and nothing else can be appealed to:
   - absolute dev-box paths into sibling repositories — `git grep -n "/home/ec2-user/projects/"` (currently 7: 6 into one sibling, 1 into another). These are provenance asides; delete the path, keep the claim.
   - any other organisation's names, workspaces, hostnames, buckets, KV/worker names, mail labels, or document filenames. **A grep only finds terms already known** — read the `.workaholic/` records and `docs/` with the question "would a stranger learn something about someone else's systems from this?", because the things that actually leak (a resource name, a label, a filename) are never on a list beforehand.
   - real addresses used as examples: `a@qmu.jp` appears ~322 times. It is our own and already public in commit authorship, so this is a judgement, not a defect — decide deliberately whether the published docs should use `you@example.com` throughout instead.
3. **Decide what `.workaholic/` publishing means.** It is committed and not ignored, so the squash publishes every ticket, story, concern, and trip record — including this one. Either accept that (they are engineering records and, once step 2 is done, carry nothing about anyone else), or exclude the directory from the published tree. Decide explicitly; do not let the default decide.
4. **Create the fresh repository and push the squashed tree.** One commit, no parents. Keep the existing private repository as the historical record — do not delete it; it is where the 881 commits legitimately continue to live.
5. **Verify the published repository from the outside**, not from the working tree: clone it fresh into a temp directory and confirm `git log --oneline | wc -l` is 1, `git log --all -S<term>` finds nothing for each term from step 2, and the tree matches what was intended.
6. **Add `.workaholic/leak-denylist`** to the new repository (git-ignored, so it must be created locally) listing the sibling repositories and any known external names. Note it is a backstop for *known* terms only — it would not have caught either item above, and it is not what makes publishing safe. Step 2's reading is.

## Quality Gate

- A fresh clone of the published repository has exactly **one commit**.
- In that clone, `git log --all -S"<each term from step 2>"` returns nothing — checked against the published remote, not the local working tree, because the working tree is not what readers get.
- `git grep -n "/home/ec2-user/projects/"` in the clone returns nothing, or only this repository's own path.
- No other organisation's workspace, cloud resource, host, or repository is named anywhere in the clone — including under `.workaholic/`, if step 3 chose to publish it.
- The step-3 and step-4 decisions (publish `.workaholic/` or not; keep the private repo as the record) are written into this ticket's Final Report with their rationale.
- Every claim the removed asides supported still stands on its own — if deleting a path or a workspace name leaves a sentence incomplete, rewrite the sentence rather than restoring the detail.

## Considerations

- **Order matters and is not recoverable:** step 1 before step 4. Publishing is a one-way door — once the squashed tree is pushed public, a leak in it needs the same GitHub Support GC dance this plan exists to avoid.
- The two history-only commits (`be5cfbd`, `fdc7791`) need no action **provided** the publication is a fresh repository. If the plan ever changes back to rewriting this repository's history in place, they become the primary target and this ticket's premise no longer holds.
- This request was raised from another repository, so its author cannot see this repo's local branches. If any of the above has already been handled on a branch, close that item rather than redoing it.

## Progress — steps 1, 2, 6 done on branch `work-20260715-163500` (2026-07-15)

**Correction to this ticket's premise.** The Overview table treats `be5cfbd` as history-only. That is
half right: `be5cfbd` carried **two leaks with different lifetimes**. The RESUME checkpoint (client KV/D1/
worker names, the real Cloudflare account id) was deleted by `fdc7791` and is genuinely history-only — a
term-by-term re-grep of the tree confirms none of it survives, so the squash does remove it. But the same
commit also put a **live Cloudflare zone list into `docs/cookbook/cloudflare.md`**, which was still on
`main` and had been copied into the shipped `qfs-cloudflare` SKILL.md. A squash would NOT have removed it.
Citing a commit as history-only is not the same as checking whether its content survives in the tree.

**Leaks found by the step-2 read sweep (four agents over tickets/archive, the rest of .workaholic, docs,
and the source tree) — all fixed in `5d8631a`:**

The values themselves are deliberately NOT reproduced below — this file publishes under the step-3
decision, so naming them here would re-leak exactly what the branch removed. They are in the git-ignored
`.workaholic/leak-denylist`, and in this branch's diff for as long as the pre-squash history exists.

| what | where | note |
| --- | --- | --- |
| a client's name, as a Gmail label carrying the 様 honorific | `driver-gmail/src/client.rs` ×4, `20260703150200-read-projection-fidelity.md` ×1 | read off live Gmail into a test fixture and its doc comments |
| a live Cloudflare zone list — three zones plus a "… 3 rows" count, i.e. the account's full inventory | `docs/cookbook/cloudflare.md`, `plugins/qfs/skills/qfs-cloudflare/SKILL.md` | the miss above; fixed in the article and regenerated via `xtask gen-skills`, never hand-edited |
| three real document names from a live My Drive listing | 4 archived tickets | one names an insurance-industry deliverable; combined with the label above it identifies the engagement |
| sibling-repo dev-box absolute paths (8) | `stories/work-20260714-111817.md`, 3 archived tickets | 6 into one sibling, 1 each into two others |

Every claim the removed details supported still stands. The Gmail fixture keeps the properties it
exercises (user label, non-ASCII name, `id != name`) under a synthetic label; the live-verification
notes still say what was verified, without naming whose data it was.

Step 1's aside now reads "verified in production against a real Slack workspace on 2026-07-11".
Step 6's `.workaholic/leak-denylist` exists and is git-ignored (`.gitignore:30` added — without it the
list of external names would itself have shipped). It records that it would have caught **none** of the
leaks above.

**Not a leak, checked:** `watchtower` appears in 83 files but is qfs's own crate — the similarly-named
D1 in the checkpoint was the owner's own, for that crate. `a@qmu.jp` (329 occurrences) is 299 ticket-frontmatter
`author:` fields, i.e. the same fact as commit authorship; left as-is deliberately, since rewriting them
to `you@example.com` would falsify authorship records to no benefit.

### Step-3 decision (owner, 2026-07-15): **publish `.workaholic/`, in full**

The development record is part of what this project is, and after the step-2 sweep it carries nothing
about anyone else. This includes this ticket itself. Accepted with it: this file names `be5cfbd`/`fdc7791`
as holding client resource names in the **retained private repository**, so a public reader learns such a
map exists — judged marginal, because that repository stays private and the names themselves are not
reproduced here.

### Step-4 decision (owner, 2026-07-15): **keep the private repository as the historical record**

Per the ticket: do not delete it; the 881 commits legitimately continue to live there. Steps 4 and 5
(create the fresh repository, push the squashed tree, verify from a fresh clone) are **owner actions and
remain open** — deliberately not performed by the drive, because publishing is a one-way door.

### Judgement calls left as-is (owner, 2026-07-15)

- `allowedHosts: ['qfs-guide.qmu.dev']` in `docs/.vitepress/config.mts` — the owner's own dev tunnel host,
  functionally required for the tunnelled docs preview. Same class as `a@qmu.jp`, not another org's data.
- `docs/query-cookbook.md:3917` uses `a@qmu.jp` in one recipe where siblings use `@acme.com`. Cosmetic.

**Remaining before the door closes:** steps 4 and 5 only. The tree is swept; re-run the step-5 verification
from a fresh clone (not the working tree) once pushed.

### Ship record (2026-07-15)

The sweep shipped as PR #42, merged to `main` (`68faaaf`). All seven pre-merge gates from
`.workaholic/deployments/github-release.md` passed and the branch-safety scan returned `pass`, so
**`main`'s tree is the intended squash source.**

The **`v0.0.71` tag was deliberately not cut on this repository** (owner decision). This repo becomes
`qfs-legacy`, so a release published from it would sit on the legacy repo while `install.sh` resolves
`qmu/qfs` — the new, empty public repository. `v0.0.71` becomes the **new repository's first release**,
cut from its first (squashed) commit.

**Step 4 therefore becomes, in order:**

1. Rename this repository to `qfs-legacy`. It stays private and keeps the 881 commits (the step-4 decision).
2. Create the new public `qmu/qfs` and push `main`'s tree as one commit, no parents.
3. Run step 5's verification **against a fresh clone of the new repository** — `git log --oneline | wc -l`
   is 1; the terms in `.workaholic/leak-denylist` return nothing; `git grep -n "/home/ec2-user/projects/"`
   returns only this repo's own path.
4. Only then tag `v0.0.71` there and let `release.yml` publish the four native tarballs.

**Known gap to expect in the new repo:** `cargo run -p xtask -- check-migrations` needs release tags, and
a fresh repo has none until step 4.4. Expect it to fail or no-op on the first commit; that is the squash's
cost, not a defect.

## Final Report — steps 4 and 5 done (2026-07-15). Only the visibility flip remains.

This file now lives in the **new** repository; the text above was written from the predecessor and its
"this repo becomes qfs-legacy" phrasing refers to that one.

**A hazard found while switching over, worth recording because it nearly undid everything.** After the
rename, the predecessor's `origin` still read `https://github.com/qmu/qfs`. GitHub redirects a renamed
repo's old URL — **but creating a new repo under the old name silently kills that redirect**, so the
predecessor's remote had come to point at the *new, empty, about-to-be-public* repository. A routine
`git push origin main` from the old checkout would have pushed all 863 commits — including the two this
ticket exists to keep out — straight into the public vessel. The remote was repointed at
`git@github.com:qmu/qfs-legacy.git` before anything else was done. **Any other checkout or worktree of the
predecessor still has the poisoned remote.**

**Step 4, as performed:**

1. Predecessor renamed to `qfs-legacy`. Private, 863 commits, remote repointed. It is the historical record.
2. The tree was exported with `git archive main` (tracked files only — `.env`, `.qfs-state/`, and the
   git-ignored `leak-denylist` are structurally excluded) and committed as one root commit, no parents.
   **The new tree's hash is identical to the predecessor `main`'s tree hash** — the strongest available
   proof the copy is faithful.
3. Pushed to `qmu/qfs`, which was **created private**. This is what made the order safe: pushing is not
   the one-way door. Flipping to public is.

**Step 5, verified against a fresh clone (not the working tree):**

| gate | result |
| --- | --- |
| `git log --oneline \| wc -l` | **1** (root commit, no parents) |
| every `.workaholic/leak-denylist` term, in tree **and** `git log --all -S` | **0 hits, all 14** |
| `git grep -n "/home/ec2-user/projects/"` | only this ticket quoting the check itself; no real sibling paths |
| `.env` / `.qfs-state/` / `leak-denylist` present? | no (`.env.example` holds an empty key only) |
| extra live-resource terms swept by hand | 0 |

One residual was removed after the first push and before any of this was public: an owner-owned D1 name
survived inside the "not a leak, checked" note above. Owner-owned and marginal, but removed for the same
reason the owner's own zone names were — consistency about live resource names. The root commit was
amended and force-pushed (owner-authorized), then **re-verified from a second fresh clone**, since the
first clone was stale and the working tree is not what readers get.

**`v0.0.71` released** from the root commit: run `29405094072`, four native tarballs plus sha256s,
`isDraft: false`. This satisfies the post-merge promotion check in `.workaholic/deployments/github-release.md`.

### The one thing left, and it is the owner's

**`qmu/qfs` is still PRIVATE.** Flipping it to public is the irreversible act this whole ticket was
written around. Everything checkable has been checked; what cannot be checked mechanically is the step-2
question — *would a stranger learn something about someone else's systems from this?* — which is why the
sweep was a read, and why the last look belongs to a person.

### Standing obligation this creates

`.workaholic/` publishes from here on. Every future ticket, story, and live-round note lands in a public
tree, and the Gmail/Drive/Slack/Cloudflare accounts that produced three of the four findings are still
live-connected. Records must describe values by **shape and location**, never quote them — including
records *about* leaks, which is the mistake this ticket's own first draft made.
