---
type: Feedback
title: Ship and document the Slack thread-reply write (replies INSERT map)
kind: instruction
source: discussion
subject: person:tamurayoshiya
created_at: 2026-09-26T14:27:34+09:00
author: a@qmu.jp
supersedes: 
---

# Ship and document the Slack thread-reply write (replies INSERT map)

Source: https://github.com/qmu/qfs/issues/140

## What happens

An agent following the qfs plugin's Slack skill cannot find a way to reply in a Slack thread, and concludes it is impossible — while a thread reply works on machines that happen to carry a hand-added map.

- The shipped Slack declaration (checked on v0.0.137 and v0.0.139) declares `/slack/{ws}/{channel}/messages/{ts}/replies` as a **read view only**. There is no INSERT map, so `insert into …/messages/<ts>/replies values (text) ('…')` answers `does not support INSERT; supported: [SELECT]`.
- On another machine the same insert works, because someone ran this once against that machine's system DB:

```qfs
CREATE MAP INSERT /slack/{ws}/{channel}/messages/{ts}/replies AS
  INSERT INTO /http/slack/chat.postMessage
  VALUES ({channel: path.channel, text: row.text, thread_ts: path.ts})
```

After that the reply lands in the thread and reads back under `…/replies` with the parent's `thread_ts` (verified 2026-09-26).
- The `qfs-slack` skill documents reading a channel and posting a root message, and says nothing about threads, so neither the replies write nor the one-time map is discoverable from the docs. (Related: #139 — `thread_ts` as a column on `…/messages` is silently dropped.)
- Thread replies existed as compiled code (`SlackNode::Replies`, v0.0.71) and were removed in the move to the declared driver (v0.0.95); the declaration kept the read view but not the write.

## Ask

1. Ship the `…/messages/{ts}/replies` INSERT map in the Slack declaration, so a thread reply works on every machine without a local `CREATE MAP`.
2. Document thread replies in the `qfs-slack` skill: read a thread, reply to it, and confirm by reading `…/replies` back.
3. Until 1 lands, the skill should at least name the one-time `CREATE MAP` so an agent does not conclude thread replies are unsupported.
