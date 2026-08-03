---
type: Feedback
title: merge-pr.sh returns the branch head, not the merge commit, so a release-on-tag ship tags off the base line
kind: concern
source: discussion
created_at: 2026-08-03T21:29:35+09:00
author: a@qmu.jp
supersedes: 
---

# merge-pr.sh returns the branch head, not the merge commit, so a release-on-tag ship tags off the base line

## Description

`workaholic:ship`'s `merge-pr.sh` returns the **branch head** as its `commit_hash`, not the merge
commit it just created on the base. Observed on the qfs v0.0.93 ship (PR #30, 2026-08-03): the script
reported `{"merged": true, "commit_hash": "97c0134"}`, but `97c0134` is `Add release notes for
work-20260801-044839` — the last commit on the claim branch — while the merge commit on `main` is
`cd4a3f0`.

That matters for a **release-on-tag** target, where the deployment contract says to tag *from the merge
commit* and Ship Flow step 7 says to target `merge-pr.sh`'s `commit_hash`. Following both literally
puts the release tag on a commit that is not on the base's first-parent line. This time it was
harmless — the two trees were byte-identical (`ed1a330…`) and the tagged commit is an ancestor of the
merge, so the published artifacts matched `main` — and moving an already-pushed tag would have been
the larger harm, so it was left in place and reported.

It is not harmless in general. Whenever the base advances between the branch's last catch-up and the
merge, the branch head's tree is **not** the merged tree, and the release would be built from
something that never existed on `main`. A squash or rebase merge widens the gap further: the branch
head is then not even an ancestor of what landed.

## How to Fix

Have `merge-pr.sh` report the commit that actually landed on the base — e.g. `gh pr view <n> --json
mergeCommit` (or `git rev-parse origin/<base>` after the post-merge fetch) — and return it as
`commit_hash`, keeping the branch head as a separate field if a caller wants it. A ship that tags
`commit_hash` then tags the merge by construction rather than by luck.
