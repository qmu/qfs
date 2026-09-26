---
type: Feedback
title: Slack INSERT drops thread_ts, so a thread reply lands in the channel
kind: instruction
source: discussion
subject: person:tamurayoshiya
created_at: 2026-09-26T14:27:29+09:00
author: a@qmu.jp
supersedes: 
---

# Slack INSERT drops thread_ts, so a thread reply lands in the channel

Source: https://github.com/qmu/qfs/issues/139

## What happens

A Slack message cannot be posted as a thread reply through qfs.

The declared Slack driver's `messages` INSERT sends only `{channel, text}` to `chat.postMessage` (see `crates/skill/assets/examples/slack_driver.qfs`, the `INSERT INTO /http/slack/chat.postMessage VALUES ({channel: path.channel, text: row.text})` line). So:

```qfs
insert into /slack/<ws>/<channel>/messages values (text, thread_ts) ('reply', '<parent ts>')
```

previews as 1 affected row, commits with `committed: true`, and the message lands **in the channel as a new root**, not in the thread. The `thread_ts` column the caller supplied is silently dropped. The thread's `messages/<ts>/replies` node is SELECT-only, so there is no other write path.

Reproduced on v0.0.139 (the latest release) on 2026-09-26: the posted message read back with `thread_ts: null` and does not appear under the parent's replies.

## Ask

1. Pass `thread_ts` through to `chat.postMessage` when the inserted row carries a non-null one (optionally `reply_broadcast` too), so an agent can reply in a thread.
2. A root post with no `thread_ts` must behave exactly as today.
3. More generally, an INSERT that names a column the declared map does not send should be **refused** (or at least warned in the preview) rather than committed with the value dropped — the silent drop is what made this look like it worked.

## Why it matters

Agents that watch a Slack channel and answer requests need to reply in the requester's thread; without it every answer becomes a new channel root and the conversation fragments.
