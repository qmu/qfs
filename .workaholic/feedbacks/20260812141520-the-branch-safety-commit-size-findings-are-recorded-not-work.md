---
type: Feedback
title: The branch-safety commit-size findings are recorded, not work
kind: insight
source: discussion
created_at: 2026-08-12T14:15:20+09:00
author: a@qmu.jp
supersedes: 20260804212551-two-commits-exceed-the-branch-safety.md
---

# The branch-safety commit-size findings are recorded, not work

Disposition after review on 2026-08-12: NOT turned into a ticket — the record itself says there is nothing to fix in the code.

It is a note about two past commits exceeding the branch-safety size threshold, kept so the ship gate's demotion (PR path rather than auto-merge) is understood rather than mistaken for a failure. The same finding recurred on PR #36 for the same honest reason: a language addition lands as parser, loader, extraction, declaration and proofs in one piece.

If the threshold is wrong for that shape of change, that is a question about the scan's policy and belongs to the workaholic plugin, not to this repository's queue.
