---
type: Feedback
title: The retired Slack webhook surface needs no ticket until inbound events are wanted
kind: insight
source: discussion
created_at: 2026-08-12T14:15:18+09:00
author: a@qmu.jp
supersedes: 20260804212551-the-slack-webhook-surface-went-with.md
---

# The retired Slack webhook surface needs no ticket until inbound events are wanted

Disposition after review on 2026-08-12: NOT turned into a ticket.

The record is accurate — parse_event/verify_signature went with the deleted compiled crate, nothing called them, and inbound Slack events are now unimplemented rather than merely unwired. But there is no work to do until inbound events are actually wanted: the concern's own How to Fix is a instruction to a future author (recover the module from git history rather than rewriting it), not a change to make now.

Leaving it as a record is the correct outcome. A ticket would be speculative work with no requester.
