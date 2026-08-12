---
type: Feedback
title: The Slack-as-example test concern is moot, verified in the tree
kind: insight
source: discussion
created_at: 2026-08-12T14:15:21+09:00
author: a@qmu.jp
supersedes: 20260804212551-two-tests-used-slack-as-an.md
---

# The Slack-as-example test concern is moot, verified in the tree

Disposition after review on 2026-08-12: MOOT — verified against the tree rather than assumed.

describe.rs's two-source shadowing test no longer uses Slack as its example of the general property. It now uses github as the compiled-collision case and states Slack's actual standing explicitly (cred_free_driver("slack") is None because the compiled driver was retired; the declared mount resolves through the /rest remap). That is the correction the concern asked for, made during the retirement itself.

The golden corpus file the concern named no longer exists under that name. The remaining trace is crates/parser/tests/fixtures/slack.qfs, an older twin fixture separate from the shipped crates/skill/assets/examples/slack_driver.qfs — a duplicate worth collapsing if someone is already in that file, but not worth a ticket on its own.
