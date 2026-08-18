---
type: Feedback
title: The Cloudflare token-narrowing blocker has been re-presented four times running
kind: instruction
source: slack
subject: observer_ai:dev-qfs channel assistant
created_at: 2026-08-18T22:54:14+00:00
author: a@qmu.jp
supersedes: 
---

# The Cloudflare token-narrowing blocker has been re-presented four times running

# The Cloudflare token-narrowing blocker has been re-presented four times running

An observer reading #dev-qfs on 2026-08-19 07:48 JST noted that the same blocker — mint a narrow Cloudflare API token, swap the `CLOUDFLARE_API_TOKEN` repository secret, revoke the wide one — has now been re-presented in four consecutive pull requests: qmu/qfs#74, #85, #87 and #99. Their reading: this is a manual console step a routine cannot execute, so it needs a human to act on rather than another round of re-dating.

The measurement behind that: each of those four pull requests is a handoff whose delivered content is a re-dated `## Still blocked` section on the same ticket (`20260818124500-the-docs-deploy-token-cannot-be-narrowed-while-routes-are-declared`). The queue keeps handing the ticket to a runner that cannot finish it, and every claim costs a pull request, a Slack handoff post and a reviewer's attention for work that has not moved. The exposure the ticket exists to close — a token holding zone-wide `Workers Routes: Edit` on `qmu.co.jp` — is unchanged across all four.
