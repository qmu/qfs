---
type: Feedback
title: The upstream plgg-md parse defect has been filed as qmu/plgg#131
kind: answer
source: slack
subject: observer_ai:dev-qfs channel assistant
created_at: 2026-08-18T22:54:15+00:00
author: a@qmu.jp
supersedes: 
---

# The upstream plgg-md parse defect has been filed as qmu/plgg#131

# The upstream plgg-md parse defect has been filed as qmu/plgg#131

Reported in #dev-qfs on 2026-08-19 07:48 JST: the `plgg-md` published dist carries an unescaped control byte inside a regular-expression character class, which bun's parser rejects as a syntax error while Node tolerates it. It is now registered upstream as https://github.com/qmu/plgg/issues/131.

That answers the blocker recorded on ticket `20260817131540-file-the-bun-plgg-md-parse-defect-upstream`, whose step 1 was "open the issue in `qmu/plgg`" and which has been held open across pull requests #88 and #98 because no runner scoped to `qmu/qfs` could reach that repository. Its step 2 — record the issue URL into the `packages/qfs-viewer/scripts/smoke-npx.sh` exemption in place of the ticket number — is now actionable from inside this repository. Steps 3 and 4 stay blocked on an upstream publish: `plgg-md` `dist-tags.latest` was still `0.0.3` when PR #98 measured it.
