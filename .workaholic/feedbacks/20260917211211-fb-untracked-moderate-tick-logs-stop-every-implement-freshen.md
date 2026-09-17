---
type: Feedback
title: [FB] Untracked /moderate tick logs stop every [Implement] freshen
kind: instruction
source: development
subject: person:tamurayoshiya
created_at: 2026-09-17T21:12:11+09:00
author: a@qmu.jp
supersedes: 
---

# [FB] Untracked /moderate tick logs stop every [Implement] freshen

kind: instruction / source: development / subject: person:tamurayoshiya

# Untracked /moderate tick logs stop every [Implement] freshen

`/moderate` writes its tick log to `.workaholic/moderations/<date>.md` and its `persist-log`
step states explicitly that the file is left in the checkout **untracked**. `/implement`'s
first step (`workaholic:drive` §1) runs `branching/scripts/sync-main.sh`, which refuses **any**
dirty tree, untracked files included. The two contracts are each internally consistent and
they deadlock at the seam.

Measured on this repository, 2026-09-17:

```
$ sync-main.sh
{"ok": false, "reason": "dirty_workspace", "branch": "main", "summary": "1 untracked"}
$ git status --porcelain
?? .workaholic/moderations/
```

The rescue script that runs next, `branching/scripts/clear-proved-residue.sh`, never deletes
untracked files (nothing on a ref proves them recoverable), so the loop cannot recover on its
own. Per the drive contract a tick whose freshen is refused never reaches the survey and ends
`pending`. The net effect: **every successful `/moderate` run stops every subsequent
`[Implement]` tick**, the same shape the 2026-09-08 mission
`clear-the-residue-the-base-already-holds-and-never-stop-silently` measured as 21 ticks and
roughly 100 minutes of idling.

## What is asked

Add one line to this repository's `.gitignore`:

```
# /moderate's tick logs are deliberately untracked; keep them out of the freshen check.
.workaholic/moderations/
```

`sync-main.sh` then passes and `/moderate`'s log stays in the checkout exactly as its own
contract intends.

## Interim measure already taken (local only, not committed)

So this server's checkout does not stay stopped until the permanent fix lands, the same line
was added to `/home/ec2-user/projects/qfs/.git/info/exclude`. That is local to this checkout:
any other checkout, and any clean clone, is still blocked. The `.gitignore` line is what makes
the fix travel.

## Also observed in the same tick (not part of this ask)

`package.json` carried uncommitted `npm init -y` boilerplate (`main: index.js`,
`license: ISC`, empty `keywords`/`author`), which stopped the freshen the same way;
`clear-proved-residue.sh` refuses it as `divergent_residue`. It was stashed rather than
discarded (`git stash list`, `drive-residue: npm-init boilerplate ...`). Pop and commit it if
the change was intentional.

Source: https://github.com/qmu/qfs/issues/123
